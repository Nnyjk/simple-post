//! Persistence layer for Simple Post.
//!
//! Layout:
//!   - `db::init` opens (or creates) the SQLite file and runs `schema.sql`.
//!     A brand-new database is left **empty** — this crate seeds nothing.
//!   - `db::with_conn` / `db::with_tx` are the only sanctioned ways to
//!     touch a connection — every CRUD module goes through one of them.
//!   - `db::ping` is a cheap liveness probe.
//!
//! Concurrency model:
//!   - WAL mode is enabled in `init`, so concurrent readers don't block
//!     each other and a single writer doesn't block readers.
//!   - Writes go through `with_tx`, which uses `BEGIN IMMEDIATE` and
//!     retries on `SQLITE_BUSY` (3 attempts, 50 ms back-off). The
//!     upgrade from DEFERRED → IMMEDIATE happens inside the transaction
//!     so we can't deadlock with another reader.
//!
//! File location:
//!   - On Windows: `%APPDATA%\com.simple-post\store.db`
//!     (`dirs::data_dir()/com.simple-post/store.db`)
//!   - The directory is created if missing; the DB file is created if
//!     missing. First boot creates the tables and stops there — rows
//!     only ever come from the user or from an MCP client.
//!
//! `db/fixtures.json` still ships in-tree, but it is **test-only** data:
//! it lives behind `#[cfg(test)] pub(crate) mod test_support` and is
//! unreachable from a production build. See `test_support::init_seeded`.
//!
//! Per-table CRUD lives in the sibling `db/*.rs` modules. They all
//! take a `&mut rusqlite::Connection` so the caller controls whether
//! the call is in a transaction.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use r2d2::ManageConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OpenFlags};

use crate::error::AppError;

// -------------------------------------------------------------------------
// Sub-modules. Public so the orchestration layer / Tauri commands can
// reach individual CRUD functions without going through an aggregator.
// -------------------------------------------------------------------------

pub mod base_url_defs;
pub mod endpoint_details;
pub mod environments;
pub mod nodes;
pub mod permissions;
pub mod preferences;
pub mod projects;

// -------------------------------------------------------------------------
// Public types.
// -------------------------------------------------------------------------

/// The shared connection pool. Cloning a `DbPool` is cheap (it's just
/// an `Arc` inside) — pass it around freely. Pool size is small (4)
/// because SQLite serialises writes anyway; bigger pools just add
/// contention without throughput.
pub type DbPool = r2d2::Pool<SqliteConnectionManager>;

// -------------------------------------------------------------------------
// Schema versioning.
// -------------------------------------------------------------------------

/// The layout this build reads and writes, stamped into
/// `PRAGMA user_version` by the tail of `schema.sql`.
///
/// **Bump this whenever `schema.sql` changes shape in a way an older
/// build's file cannot satisfy** — dropping or renaming a column, or
/// splitting one table into several. Purely additive changes (a new
/// table, a new nullable column with a default) do not need a bump:
/// `CREATE TABLE IF NOT EXISTS` already handles them, and old files keep
/// working because readers tolerate the missing column.
///
/// v3 is a *content* change rather than a column change, and it needs the
/// bump anyway: `endpoint_details.auth_json` stores `serde_json` output
/// verbatim, so dropping the `type` → `kind` rename in `domain.rs` left
/// every existing row shaped `{"type":"bearer",…}` — including the `'{}'`
/// default, which no longer deserializes because `kind` is required. A
/// v2 file is unreadable, not merely stale, and the rebuild path is the
/// project's answer to that (see [`ensure_current_schema`]).
///
/// Keep the history comment at the bottom of `schema.sql` in sync.
pub const SCHEMA_VERSION: i64 = 3;

// -------------------------------------------------------------------------
// Public API.
// -------------------------------------------------------------------------

/// Open (or create) the DB at `<data_dir>/store.db`, bring it up to
/// [`SCHEMA_VERSION`], and return the pool.
///
/// `data_dir` is typically `dirs::data_dir().unwrap().join("com.simple-post")`.
/// The directory is created if it doesn't exist.
///
/// A file written by an older layout is **replaced, not upgraded** —
/// see [`ensure_current_schema`]. That is logged rather than recorded:
/// the app has no migration path, and a rebuild is a developer's
/// experience on a pre-release schema, not something to interrupt the
/// user about on every launch.
pub fn init(data_dir: &Path) -> Result<DbPool, AppError> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("store.db");

    // Before any connection is handed out — a pool opened on a stale
    // file would let reads succeed against tables this build no longer
    // understands.
    let rebuilt = ensure_current_schema(&db_path)?;

    let manager = SqliteConnectionManager::file(&db_path)
        .with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)
        // WAL across pool connections: each new connection inherits the
        // journal mode set by the first connection. We set it here so a
        // cold start doesn't race a reader against a writer.
        .with_init(|c| {
            // PRAGMA can't carry user params safely across connections,
            // so we set WAL/synchronous/FK/busy_timeout via the canonical
            // raw-statement path.
            c.execute_batch(
                "PRAGMA journal_mode = WAL;\
                 PRAGMA synchronous = NORMAL;\
                 PRAGMA foreign_keys = ON;\
                 PRAGMA busy_timeout = 3000;",
            )
        });

    // Eagerly probe the file so a corrupt / read-only DB fails fast
    // before the caller has wired up the rest of the app.
    let mut probe = manager.connect()?;
    migrate(&mut probe)?;
    drop(probe);

    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(manager)
        .map_err(AppError::Pool)?;

    // Smoke-test the pool: the first checkout from a freshly built pool
    // forces the manager to actually open a connection, which is where
    // a locked file / bad path would surface.
    ping(&pool)?;

    if rebuilt {
        tracing::warn!(
            "database schema v{SCHEMA_VERSION}: replaced an older store.db at {} — \
             its contents are gone",
            db_path.display()
        );
    }

    Ok(pool)
}

/// Run a closure against a pooled connection (read or write).
/// Read-only callers can use this; write callers should prefer
/// `with_tx` so the SQL is wrapped in a transaction.
///
/// The connection's PRAGMAs (foreign_keys, journal_mode, …) are
/// applied by the pool's `with_init` hook above, so each checkout
/// gets a correctly-configured handle.
pub fn with_conn<F, R>(pool: &DbPool, f: F) -> Result<R, AppError>
where
    F: FnOnce(&mut Connection) -> Result<R, AppError>,
{
    let mut conn = pool.get()?;
    f(&mut conn)
}

/// Run a closure inside a write transaction with `SQLITE_BUSY` retry.
///
/// On a contended DB the `BEGIN IMMEDIATE` (or the first write inside
/// the transaction) can fail with `SQLITE_BUSY`. We retry up to 3
/// times with 50 ms between attempts — long enough for the other
/// writer to commit, short enough to stay snappy. After 3 failures we
/// surface the error as `AppError::Db`.
///
/// Note: rusqlite's `transaction()` helper uses `BEGIN DEFERRED` by
/// default, which can silently turn into `SQLITE_BUSY` on the *first
/// write*. We want `BEGIN IMMEDIATE` up front so the lock is taken
/// before any reads happen, matching the semantics callers expect.
pub fn with_tx<F, R>(pool: &DbPool, f: F) -> Result<R, AppError>
where
    F: Fn(&mut Connection) -> Result<R, AppError>,
{
    const RETRIES: u32 = 3;
    const BACKOFF: Duration = Duration::from_millis(50);

    let mut last_err: Option<AppError> = None;
    for attempt in 0..RETRIES {
        let mut conn = pool.get()?;
        match run_immediate_tx(&mut conn, &f) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let busy = matches!(
                    &e,
                    AppError::Sqlite(rusqlite::Error::SqliteFailure(err, _))
                        if err.code == rusqlite::ErrorCode::DatabaseBusy
                );
                if !busy {
                    return Err(e);
                }
                last_err = Some(e);
                if attempt + 1 < RETRIES {
                    std::thread::sleep(BACKOFF);
                }
            }
        }
    }
    Err(match last_err {
        Some(e) => AppError::Db(format!("sql busy after {RETRIES} retries: {e}")),
        None => AppError::Db("sql busy".into()),
    })
}

/// Run the embedded `schema.sql`, bring an existing store up to the
/// current column set, then repair the rows either predecessor layout
/// wrote. Idempotent — safe to call on every boot.
///
/// Every `CREATE` in the schema is `IF NOT EXISTS`, so re-running against
/// an already-migrated database changes nothing. The parent-matrix
/// triggers are the deliberate exception: they are dropped and rebuilt,
/// because their bodies *are* the contract and `IF NOT EXISTS` would
/// leave an existing database enforcing a matrix the application layer
/// no longer holds. **No rows are inserted**: a fresh install starts
/// empty and is populated by the user or by an MCP client. Test-only
/// seed data lives in `test_support`.
pub fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    conn.execute_batch(SCHEMA_SQL)?;
    // Before the data migration, never inside `SCHEMA_SQL`: that pass
    // reads and writes `module_base_urls_json`, `module_id` and
    // `is_default`, and a store old enough to lack them has no way to
    // hold the values the pass is there to move. `SCHEMA_SQL`'s
    // `CREATE TABLE IF NOT EXISTS` is a no-op on exactly those stores,
    // which is why the columns have to be re-added here.
    add_missing_columns(conn)?;
    migrate_base_urls_into_value_layer(conn)
}

/// Columns that exist in a `store.db` written by some build of this
/// project but not in every one.
///
/// `CREATE TABLE IF NOT EXISTS` is a no-op against a table that already
/// exists, so a column added to `schema.sql` later never reaches a user
/// who already has a file. Re-adding it here is the only way that file
/// ever sees it.
///
/// This deliberately does **not** bump `SCHEMA_VERSION`. That constant is
/// not a migration counter: [`ensure_current_schema`] reacts to a mismatch
/// by *deleting* the database, so a patch release that widened a table
/// would wipe every user's projects, environments and tree. The `IF NOT
/// EXISTS` in `schema.sql` and this additive pass are the whole
/// "purely additive changes need no bump" promise the version doc makes.
///
/// Idempotency comes from asking SQLite rather than from bookkeeping: a
/// column already present is skipped, so this runs on every boot and does
/// nothing the second time.
///
/// The first entry is the live value layer. The other two are the retired
/// per-module service model, kept because the data migration has to be
/// able to *read* that layout — a store that had the columns added by
/// [`SCHEMA_SQL`]'s `CREATE TABLE` and a store that had them added by this
/// list are the same shape to the migration, and both must be repairable.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    (
        "environments",
        "module_base_urls_json",
        "TEXT NOT NULL DEFAULT '{}'",
    ),
    (
        "base_url_definitions",
        "module_id",
        // Nullable because `ALTER TABLE ... ADD COLUMN` can only append a
        // column with a default, and a foreign key needs one; every row
        // of this shape lands with the column NULL anyway.
        "TEXT REFERENCES nodes(id) ON DELETE CASCADE",
    ),
    (
        "base_url_definitions",
        "is_default",
        "INTEGER NOT NULL DEFAULT 0",
    ),
];

/// Add each [`ADDED_COLUMNS`] entry whose table is missing that column.
///
/// The `PRAGMA table_info` scan is deliberately literal: a table that
/// `schema.sql` has not created yet simply yields no rows, which leaves it
/// alone rather than failing the whole boot.
fn add_missing_columns(conn: &mut Connection) -> Result<(), AppError> {
    for (table, column, decl) in ADDED_COLUMNS {
        if table_has_column(conn, table, column)? {
            continue;
        }
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
        tracing::info!("schema: added {table}.{column} to an existing store");
    }
    Ok(())
}

/// Does `table` already carry a column called `column`?
fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

// -------------------------------------------------------------------------
// Data migration — service list back to the project, values into
// (environment × module × service).
//
// This is data repair, not a version change. `SCHEMA_VERSION` is untouched
// on purpose (bumping it makes `ensure_current_schema` delete the store),
// so the columns arrive through [`ADDED_COLUMNS`] and this pass repairs
// the rows the current model cannot read.
//
// **Two predecessor layouts are live in the wild**, because a build that
// put the service list on the module shipped and was run:
//
// - **shape B** (that build): one service row per (module × name), values
//   in `base_urls_json[clone_id]`, ★ per module in `is_default`.
// - **shape A** (before it): one project-wide service list with
//   `module_id` NULL, values in `base_urls_json[def_id]` plus an optional
//   per-module override in `module_base_urls_json[def_id][module_id]`.
//
// Which one a store is gets decided per project by the evidence, never
// assumed: `module_id` non-NULL can only be shape B, and a store with no
// such row but a populated `base_urls_json` is shape A. A store written by
// this build has neither, so the pass is a no-op.
// -------------------------------------------------------------------------

/// A service row as either predecessor layout wrote it. `module_id` is the
/// shape discriminator: `Some` means the module-owned model, `None` the
/// project-owned one.
struct ServiceRow {
    id: String,
    name: String,
    module_id: Option<String>,
    sort_order: i64,
    /// The ★ flag the module-owned model kept per row. Always false in
    /// shape A — that layout had no per-row flag at all.
    is_default: bool,
}

/// One module of the project. Only what "the first module" means: display
/// order, then creation, then id as the final tie-break so the choice is
/// stable across runs rather than dependent on scan order.
struct ModuleRow {
    id: String,
    sort_order: i64,
    created_at: i64,
}

impl ModuleRow {
    /// Display order, as `(sort_order, created_at, id)`.
    fn rank(&self) -> (i64, i64, &str) {
        (self.sort_order, self.created_at, self.id.as_str())
    }
}

/// One environment with **both** URL layers readable. This is the only
/// reader of `base_urls_json` left in the crate, and it reads
/// `module_base_urls_json` because a shape A store's override layer is
/// part of the answer.
struct EnvRow {
    id: String,
    /// `def_id -> url`. Read as the shape A fallback and as the shape B
    /// source; never written except to be emptied once shape B is done
    /// with it (see [`migrate_project`]).
    base_urls: std::collections::BTreeMap<String, String>,
    /// `def_id -> module_id -> url`, the live layer.
    module_base_urls:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// Projects with rows this pass still has to touch.
///
/// Two independent kinds of evidence, unioned:
///
/// - a service that still names a module ⇒ shape B;
/// - an environment that still carries service-level URLs ⇒ shape A.
///
/// A project with neither is already in the current shape and is left
/// strictly alone, which is what makes the pass free on every boot after
/// the first.
fn pending_projects(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut pending: std::collections::BTreeSet<String> = Default::default();

    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT project_id FROM base_url_definitions \
             WHERE module_id IS NOT NULL",
        )?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?;
        pending.extend(ids);
    }

    for env in read_envs_needing_repair(conn)? {
        pending.insert(env);
    }

    Ok(pending.into_iter().collect())
}

/// Project ids owning an environment whose `base_urls_json` still holds
/// something. Parsed rather than compared against `'{}'` so a blob that
/// differs only in whitespace or key order still counts as evidence.
fn read_envs_needing_repair(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT project_id, base_urls_json FROM environments \
         WHERE base_urls_json IS NOT NULL AND base_urls_json <> '{}'",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (project_id, raw) = r?;
        match serde_json::from_str::<std::collections::BTreeMap<String, String>>(&raw) {
            Ok(map) if !map.is_empty() => out.push(project_id),
            Ok(_) => {}
            // Unreadable means there is nothing to carry across: the
            // alternative is refusing to boot over a column this build
            // already treats as dead.
            Err(e) => tracing::warn!(
                "environments.base_urls_json is not readable ({e}); \
                 migrating this environment without its service-level URLs"
            ),
        }
    }
    Ok(out)
}

/// Move every project's services back onto the project and every URL into
/// `module_base_urls_json[def_id][module_id]`.
///
/// One transaction for all projects: a crash mid-way rolls the whole pass
/// back and the next boot redoes it from the same starting state, so there
/// is no half-migrated shape to detect.
fn migrate_base_urls_into_value_layer(conn: &mut Connection) -> Result<(), AppError> {
    let pending = pending_projects(conn)?;
    if pending.is_empty() {
        return Ok(());
    }

    // `BEGIN IMMEDIATE` / `COMMIT` spelled out rather than
    // `conn.transaction()`, for the same reason [`run_immediate_tx`] does
    // it: `rusqlite::Transaction` only hands back an immutable borrow of
    // the connection, and every helper below takes the same `&mut
    // Connection` the rest of this module takes. IMMEDIATE rather than the
    // deferred default so a second boot racing this one fails at BEGIN
    // instead of half way through the rewrite.
    conn.execute_batch("BEGIN IMMEDIATE")?;
    for project_id in &pending {
        if let Err(e) = migrate_project(conn, project_id) {
            // Best-effort rollback — the original error is the useful one.
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    conn.execute_batch("COMMIT")?;
    tracing::info!(
        "schema: moved {} project service list(s) back onto the project \
         and their URLs into module_base_urls",
        pending.len()
    );
    Ok(())
}

/// The per-project half of [`migrate_base_urls_into_value_layer`].
fn migrate_project(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    let modules = read_modules(conn, project_id)?;
    let services = read_services(conn, project_id)?;
    if services.is_empty() {
        return Ok(());
    }
    let envs = read_envs(conn, project_id)?;

    // Shape B is "some row still owns a module". Deciding per project is
    // what lets a store that was never touched by the module-owned build
    // take the cheaper, id-preserving path — and it is the only evidence
    // available, since both layouts leave `base_urls_json` populated.
    let module_owned = services.iter().any(|s| s.module_id.is_some());

    // The project's own ★ is read before anything rewrites it: shape A
    // keeps it (it is what its endpoints already resolved against) and
    // shape B has none left to keep (that model moved ★ onto the module).
    let star_before: Option<String> = conn.query_row(
        "SELECT default_base_url_definition_id FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )?;

    let canonical = pick_canonical(&services, &modules, module_owned);
    // clone id -> canonical id, for shape B only: an endpoint that named
    // a per-module clone has to be re-pointed at the row that survives.
    let survivor_of: std::collections::HashMap<&str, &str> = canonical
        .iter()
        .filter(|c| !c.replaced.is_empty())
        .flat_map(|c| {
            c.replaced
                .iter()
                .map(move |old| (old.as_str(), c.id.as_str()))
        })
        .collect();

    write_value_layer(conn, &envs, &modules, &canonical, &services, module_owned)?;

    if module_owned {
        remap_endpoints_onto_canonical(conn, project_id, &survivor_of)?;
    }

    // ★. Shape A keeps whatever the project already pointed at (a NULL
    // stays NULL: inventing a default would give endpoints that resolved
    // to `''` a host they never had). Shape B has to choose, because the
    // per-module ★ it used to live on is going away — see
    // [`choose_star_for_module_owned`].
    let star = if module_owned {
        choose_star_for_module_owned(&services, &modules, &canonical)
    } else {
        star_before.filter(|d| canonical.iter().any(|c| c.id == *d))
    };
    conn.execute(
        "UPDATE projects SET default_base_url_definition_id = ?1 WHERE id = ?2",
        params![star, project_id],
    )?;

    // Retire the per-module clones, then neutralise the two columns the
    // model they belonged to used. Deleting after the value layer is
    // written is what keeps the ids resolvable while it is being read.
    for c in &canonical {
        for old in &c.replaced {
            conn.execute(
                "DELETE FROM base_url_definitions WHERE id = ?1",
                params![old],
            )?;
        }
    }
    conn.execute(
        "UPDATE base_url_definitions SET module_id = NULL, is_default = 0 \
         WHERE project_id = ?1",
        params![project_id],
    )?;

    // Shape B only: `base_urls_json` was the value layer, and every value
    // in it is now in `module_base_urls_json` — each clone's URL under its
    // own module, each leftover key pointing at a row that no longer
    // exists. Leaving it would not merely be untidy: the next boot sees a
    // store with no `module_id` left, decides it is shape A, and expands
    // `base_urls[canonical]` into every module's empty cell — filling
    // module B with module A's host, on every boot, forever. Clearing it
    // is what makes this shape idempotent. Shape A is left alone: its
    // keys are its own def ids, it re-derives byte-identical values on
    // every later run, and it is the one layout where the stale blob is
    // genuinely harmless.
    if module_owned {
        conn.execute(
            "UPDATE environments SET base_urls_json = '{}' WHERE project_id = ?1",
            params![project_id],
        )?;
    }
    Ok(())
}

/// A service that survives the migration, plus the ids it stands in for.
struct CanonicalService {
    id: String,
    name: String,
    sort_order: i64,
    /// Ids of the per-module clones this row replaces (shape B only).
    /// Empty means nothing was collapsed, which is every row in shape A.
    replaced: Vec<String>,
}

/// Decide which row represents each service name.
///
/// Shape A: every row is already project-wide, so every row survives and
/// nothing is collapsed. Renaming is therefore not a merge, and two rows
/// sharing a `name` are a state the application itself cannot create.
///
/// Shape B: one row per (module × name), and the project's service list
/// has to end up with one row per name. The survivor is the one under the
/// **first** module — display order, then creation, then id — because
/// that is the module whose configuration the project had the most of,
/// and because picking anything else would need a rule for "which module
/// wins" that has no basis in the data. Within one module the tie-break
/// is the row's own display order.
///
/// A row with no module at all cannot be "the first module's", so it is
/// ranked behind every real module: it wins only when it is the sole row
/// for that name, which is the case a mixed store leaves behind when the
/// module-owned build created new services before the old ones were
/// collapsed.
fn pick_canonical(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    module_owned: bool,
) -> Vec<CanonicalService> {
    if !module_owned {
        return services
            .iter()
            .map(|s| CanonicalService {
                id: s.id.clone(),
                name: s.name.clone(),
                sort_order: s.sort_order,
                replaced: Vec::new(),
            })
            .collect();
    }

    let rank_of = |module_id: Option<&str>| -> (u8, (i64, i64, &str)) {
        match module_id.and_then(|m| modules.iter().find(|x| x.id == m)) {
            // Owned rows sort ahead of unowned leftovers.
            Some(m) => (0, m.rank()),
            None => (1, (0, 0, "")),
        }
    };

    let mut by_name: std::collections::BTreeMap<&str, &ServiceRow> = Default::default();
    for s in services {
        let entry = by_name.entry(s.name.as_str()).or_insert(s);
        let better = rank_of(s.module_id.as_deref()) < rank_of(entry.module_id.as_deref())
            // Same module (or both unowned): fall back to the row's own
            // display order so the choice does not depend on scan order.
            || (rank_of(s.module_id.as_deref()) == rank_of(entry.module_id.as_deref())
                && (s.sort_order, s.id.as_str()) < (entry.sort_order, entry.id.as_str()));
        if better {
            *entry = s;
        }
    }

    by_name
        .values()
        .map(|keep| CanonicalService {
            id: keep.id.clone(),
            name: keep.name.clone(),
            sort_order: keep.sort_order,
            replaced: services
                .iter()
                .filter(|s| s.name == keep.name && s.id != keep.id)
                .map(|s| s.id.clone())
                .collect(),
        })
        .collect()
}

/// Which service becomes the project's ★ under the module-owned model.
///
/// The first module's ★, because that is the one the project most
/// plausibly meant: its requests went out through it. **When two modules
/// disagree there is no right answer** — the module-owned model gave each
/// module an independent default and nothing ever tied them together, so
/// there is no inheritance to follow and no majority to consult. "First
/// module wins" is a stated tie-break, not a reconstruction of intent;
/// the alternative (leaving ★ unset) would silently strip every endpoint
/// that named no service of a base URL it had.
///
/// A first module with no ★ of its own falls back to the service with the
/// smallest `sort_order`, which is the one the picker shows first — the
/// same answer `base_url_defs::set_default` would produce if the user
/// picked "the first one".
fn choose_star_for_module_owned(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
) -> Option<String> {
    let first_module = modules.iter().min_by(|a, b| a.rank().cmp(&b.rank()));
    if let Some(first) = first_module {
        let stars: Vec<&ServiceRow> = services
            .iter()
            .filter(|s| s.is_default && s.module_id.as_deref() == Some(first.id.as_str()))
            .collect();
        if let Some(pick) = stars
            .iter()
            .min_by(|a, b| (a.sort_order, &a.id).cmp(&(b.sort_order, &b.id)))
        {
            // The ★ may itself be a clone, in which case the project's
            // default has to name the row that survived, not the row that
            // is about to be deleted.
            if let Some(keep) = canonical
                .iter()
                .find(|c| c.name == pick.name)
                .map(|c| c.id.clone())
            {
                return Some(keep);
            }
        }
    }
    canonical
        .iter()
        .min_by_key(|c| c.sort_order)
        .map(|c| c.id.clone())
}

/// Copy every live URL into `module_base_urls_json[def_id][module_id]`.
///
/// The cell's value is whatever the **old** model resolved for that exact
/// (service × module) pair, and nothing else:
///
/// - shape A: the per-module override if the environment has one, else
///   the environment-level URL — which is precisely what the previous
///   resolution did, in that order.
/// - shape B: the URL of the clone of the same name under that module.
///   Absent for a module that never had that service, which is the point:
///   an empty cell means "this module has no address for this service" and
///   must not be filled from a sibling module.
///
/// A cell that already holds a value keeps it, so the pass is additive and
/// re-running changes nothing. When neither source has a value the cell is
/// left absent rather than written empty — "not set" is a state the UI
/// shows and the resolver treats as no base URL, and materialising it as
/// `""` would make the two indistinguishable in the stored data.
fn write_value_layer(
    conn: &mut Connection,
    envs: &[EnvRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
    services: &[ServiceRow],
    module_owned: bool,
) -> Result<(), AppError> {
    for env in envs {
        let mut cells = env.module_base_urls.clone();
        let mut changed = false;

        for c in canonical {
            for m in modules {
                if cells.get(&c.id).is_some_and(|per| per.contains_key(&m.id)) {
                    continue;
                }
                let url = if module_owned {
                    services
                        .iter()
                        .find(|s| s.name == c.name && s.module_id.as_deref() == Some(m.id.as_str()))
                        .and_then(|s| env.base_urls.get(&s.id))
                } else {
                    env.module_base_urls
                        .get(&c.id)
                        .and_then(|per| per.get(&m.id))
                        .or_else(|| env.base_urls.get(&c.id))
                };
                if let Some(url) = url {
                    cells
                        .entry(c.id.clone())
                        .or_default()
                        .insert(m.id.clone(), url.clone());
                    changed = true;
                }
            }
        }

        if changed {
            conn.execute(
                "UPDATE environments SET module_base_urls_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(&cells)?, env.id],
            )?;
        }
    }
    Ok(())
}

/// Point every endpoint in the project at the surviving row for the
/// service it named.
///
/// Shape B only — in shape A the ids never change. An endpoint naming
/// nothing keeps naming nothing: it resolves through the project ★, which
/// is re-pointed in the same pass.
///
/// Endpoints naming an id this project does not own are left alone. That
/// is a cross-project reference, which the write paths reject; it is not
/// something a data pass should silently rewrite.
fn remap_endpoints_onto_canonical(
    conn: &mut Connection,
    project_id: &str,
    survivor_of: &std::collections::HashMap<&str, &str>,
) -> Result<(), AppError> {
    if survivor_of.is_empty() {
        return Ok(());
    }
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT e.node_id, e.base_url_definition_id \
             FROM endpoint_details e \
             JOIN nodes n ON n.id = e.node_id \
             WHERE n.root_id = ?1 AND e.base_url_definition_id IS NOT NULL \
             ORDER BY e.node_id ASC",
        )?;
        let out = stmt
            .query_map(params![project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, _>>()?;
        out
    };
    for (endpoint_id, named) in rows {
        let Some(keep) = survivor_of.get(named.as_str()) else {
            continue;
        };
        conn.execute(
            "UPDATE endpoint_details SET base_url_definition_id = ?1 WHERE node_id = ?2",
            params![keep, endpoint_id],
        )?;
    }
    Ok(())
}

fn read_modules(conn: &mut Connection, project_id: &str) -> Result<Vec<ModuleRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, sort_order, created_at FROM nodes \
         WHERE root_id = ?1 AND type = 'module' \
         ORDER BY sort_order ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ModuleRow {
            id: row.get(0)?,
            sort_order: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_services(conn: &mut Connection, project_id: &str) -> Result<Vec<ServiceRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, module_id, sort_order, is_default \
         FROM base_url_definitions \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ServiceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            module_id: row.get(2)?,
            sort_order: row.get(3)?,
            is_default: row.get::<_, i64>(4)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_envs(conn: &mut Connection, project_id: &str) -> Result<Vec<EnvRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, base_urls_json, module_base_urls_json FROM environments \
         WHERE project_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        let base_urls: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&row.get::<_, String>(1)?)
                .map_err(|e| json_col_err("base_urls_json", e))?;
        let module_base_urls: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, String>,
        > = match row.get::<_, String>(2) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| json_col_err("module_base_urls_json", e))?
            }
            Err(_) => Default::default(),
        };
        Ok(EnvRow {
            id: row.get(0)?,
            base_urls,
            module_base_urls,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Bring the on-disk database up to [`SCHEMA_VERSION`], replacing it
/// when it predates this build's layout.
///
/// Returns `true` when the file was deleted and will be recreated, so
/// the caller can surface the data loss.
///
/// **There is no migration path.** v0.1 has never shipped to real
/// users, so a data-migration framework would be speculative weight
/// guarding a population of one. A pre-versioned file (the original
/// `modules` / `collections` / `endpoints` three-table layout) is
/// replaced instead of upgraded. When that stops being true, this
/// function is the one place that has to change.
fn ensure_current_schema(db_path: &Path) -> Result<bool, AppError> {
    if !db_path.exists() {
        // Fresh install — `migrate` creates the file and stamps it.
        return Ok(false);
    }
    let found = read_user_version(db_path)?;
    if found == SCHEMA_VERSION {
        return Ok(false);
    }
    if found > SCHEMA_VERSION {
        // The file was written by a *newer* build. Recreating it would
        // destroy data this build has no excuse destroying.
        return Err(AppError::Validation(format!(
            "database at {} is schema v{found}, but this build only understands \
             v{SCHEMA_VERSION}. Update Simple Post, or point \
             SIMPLE_POST_DATA_DIR somewhere else to start a separate workspace.",
            db_path.display()
        )));
    }
    remove_db_files(db_path)?;
    Ok(true)
}

/// Read `PRAGMA user_version` without taking a write lock.
///
/// Opened read-only on purpose: `init` calls this before the pool
/// exists, and a read-write handle would create the file we're about
/// to decide whether to delete.
fn read_user_version(db_path: &Path) -> Result<i64, AppError> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Delete the database and its WAL sidecars.
///
/// The sidecars are not optional cleanup. Leaving `store.db-wal` behind
/// while `migrate` creates a fresh `store.db` lets SQLite replay the
/// old write-ahead log into the new file — which is how a "clean"
/// rebuild comes back still holding the data it was supposed to drop.
fn remove_db_files(db_path: &Path) -> Result<(), AppError> {
    for suffix in ["", "-wal", "-shm"] {
        let path = if suffix.is_empty() {
            db_path.to_path_buf()
        } else {
            let mut name = db_path.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            // Another process may have cleaned up first, or a failed
            // open may have left the sidecar absent. Both are fine.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // On Windows this is the shape a file held open by another
            // process takes — a second `serve-web` or a stray daemon
            // owns the handle. Failing loudly beats half-rebuilding.
            Err(e) => {
                return Err(AppError::Io(std::io::Error::new(
                    e.kind(),
                    format!("cannot remove {}: {e}", path.display()),
                )))
            }
        }
    }
    Ok(())
}

/// Liveness probe. `SELECT 1` round-trips a connection from the pool.
/// Useful at startup and from a Tauri command for the health badge.
pub fn ping(pool: &DbPool) -> Result<(), AppError> {
    let conn = pool.get()?;
    conn.execute_batch("SELECT 1")?;
    Ok(())
}

// -------------------------------------------------------------------------
// Internals.
// -------------------------------------------------------------------------

/// Run `BEGIN IMMEDIATE`, dispatch to the caller, COMMIT on success
/// or ROLLBACK on error. Returns the caller's value or error.
///
/// This is split out from `with_tx` so the retry loop can borrow the
/// closure without a generic lifetime argument; `Fn` (not `FnOnce`)
/// is used so we can call repeatedly without moving the closure.
fn run_immediate_tx<F, R>(conn: &mut Connection, f: &F) -> Result<R, AppError>
where
    F: Fn(&mut Connection) -> Result<R, AppError>,
{
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f(conn) {
        Ok(v) => {
            conn.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            // Best-effort rollback — if even ROLLBACK fails we keep the
            // original error; the connection will be dropped / reused.
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// -------------------------------------------------------------------------
// Embedded static asset — kept in-tree so the binary doesn't need a
// filesystem lookup at runtime. `include_str!` makes the asset
// participate in `cargo build` rebuilds when it changes.
// -------------------------------------------------------------------------

const SCHEMA_SQL: &str = include_str!("schema.sql");

// -------------------------------------------------------------------------
// Test-only fixture loading.
//
// Everything in this module is `#[cfg(test)]`: it compiles into
// `cargo test` and nowhere else. `db::init` has no path in here, so a
// production build can never insert seed rows — a fresh install starts
// empty. `fixtures.json` remains in-tree purely as test data.
// -------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::{ResourceNode, ResourceNodeType};
    use rusqlite::params;
    use serde::Deserialize;

    /// The bundled fixture file. Test data only — never read by `db::init`.
    const FIXTURES_JSON: &str = include_str!("fixtures.json");

    /// Parse the bundled fixtures. Panicking here is correct: a malformed
    /// `fixtures.json` is a repository defect, not a runtime condition.
    fn fixtures() -> Fixtures {
        serde_json::from_str(FIXTURES_JSON).expect("fixtures.json must parse")
    }

    /// Open a database at `dir` and load the bundled fixture set into it.
    ///
    /// This is what the `#[path]`-mounted test modules use in place of the
    /// old implicit seeding: `init` used to seed on every call, so the
    /// per-module `fresh_pool()` helpers relied on that side effect. Seeding
    /// is now an explicit, visible step rather than a `db::init` surprise.
    pub(crate) fn init_seeded(dir: &Path) -> DbPool {
        let pool = init(dir).expect("init");
        seed(&pool);
        pool
    }

    /// Load the bundled fixture set into an already-initialised database.
    ///
    /// `with_conn`, not `with_tx`: `seed_fixtures` opens its own transaction
    /// so the whole fixture set lands atomically, and nesting a `BEGIN
    /// IMMEDIATE` around it fails with "cannot start a transaction within a
    /// transaction".
    pub(crate) fn seed(pool: &DbPool) {
        with_conn(pool, |c| seed_fixtures(c, &fixtures())).expect("seed fixtures");
    }

    /// Top-level shape of `fixtures.json`. We only deserialize the fields
    /// the seeder actually needs; the file can grow extra sections without
    /// breaking older binaries (provided they have defaults).
    #[derive(Debug, Deserialize)]
    struct Fixtures {
        /// Fixed clock the fixture set is stamped with. Deterministic, so
        /// a seeded database is byte-identical between runs.
        #[serde(default)]
        now: i64,
        #[serde(default)]
        projects: Vec<FixtureProject>,
        #[serde(default)]
        modules: Vec<FixtureModule>,
        #[serde(default)]
        collections: Vec<FixtureCollection>,
        #[serde(default)]
        endpoints: Vec<FixtureEndpoint>,
        #[serde(default)]
        environments: Vec<FixtureEnvironment>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureProject {
        id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        base_url_definitions: Vec<FixtureBaseUrlDef>,
        #[serde(default)]
        default_base_url_definition_id: Option<String>,
        #[serde(default)]
        base_url: Option<String>,
        color: String,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureBaseUrlDef {
        id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureModule {
        id: String,
        project_id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        sort_order: i64,
        /// UI-only expand state. Carried in the fixture so a future
        /// renderer test can read a realistic tree, never persisted.
        #[serde(default)]
        #[allow(dead_code)]
        expanded: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureCollection {
        id: String,
        module_id: String,
        #[serde(default)]
        parent_collection_id: Option<String>,
        name: String,
        #[serde(default)]
        description: Option<String>,
        sort_order: i64,
        /// UI-only expand state; see [`FixtureModule::expanded`].
        #[serde(default)]
        #[allow(dead_code)]
        expanded: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEndpoint {
        id: String,
        collection_id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        method: String,
        url: String,
        #[serde(default)]
        environment_id: Option<String>,
        #[serde(default)]
        base_url_definition_id: Option<String>,
        #[serde(default)]
        base_url_override: Option<String>,
        #[serde(default)]
        params: Vec<serde_json::Value>,
        #[serde(default)]
        headers: Vec<serde_json::Value>,
        body: serde_json::Value,
        auth: serde_json::Value,
        #[serde(default)]
        docs: Option<String>,
        #[serde(default)]
        notes: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
        sort_order: i64,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEnvironment {
        id: String,
        project_id: String,
        name: String,
        /// `def_id -> module_id -> url`, the live value layer. The
        /// retired `base_urls_json` is not seeded at all: a fixture
        /// database is a current-model store, and leaving a populated
        /// legacy column behind would make every seeded test boot run the
        /// data migration.
        #[serde(default)]
        module_base_urls: serde_json::Value,
        #[serde(default)]
        variables: serde_json::Value,
        is_active: bool,
    }

    /// Insert one row of the tree.
    ///
    /// The three fixture sections collapse onto this one shape — the
    /// only thing that varies is `kind` and whether `parent_id` is set.
    /// Takes a real [`ResourceNode`] rather than a loose column list so
    /// the fixture's translation stays a handful of struct literals
    /// instead of a call with ten positional arguments.
    fn insert_node(tx: &rusqlite::Transaction<'_>, n: &ResourceNode) -> Result<(), AppError> {
        tx.execute(
            "INSERT INTO nodes \
         (id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at) \
         VALUES (?,?,?,?,?,?,?,?,?)",
            params![
                n.id,
                n.root_id,
                n.parent_id,
                n.kind.as_str(),
                n.name,
                n.description,
                n.sort_order,
                n.created_at,
                n.updated_at
            ],
        )?;
        Ok(())
    }

    /// Insert all rows from the fixtures file. Walks the dependency graph
    /// in topological order: projects → base_url_defs + modules + envs →
    /// collections → endpoints. Foreign keys stay satisfied without
    /// `PRAGMA defer_foreign_keys`.
    fn seed_fixtures(conn: &mut Connection, fx: &Fixtures) -> Result<(), AppError> {
        let tx = conn.transaction()?;

        for p in &fx.projects {
            tx.execute(
            "INSERT INTO projects \
             (id, name, description, default_base_url_definition_id, color, created_at, updated_at) \
             VALUES (?,?,?,?,?,?,?)",
            params![
                p.id,
                p.name,
                p.description,
                p.default_base_url_definition_id,
                p.color,
                p.created_at,
                p.updated_at,
            ],
        )?;
            // baseUrlDefinitions are stored as a JSON-ish blob on the project
            // row (no schema column) — actually we *do* persist each def into
            // its own table so the foreign-key chain into endpoints works.
            for (i, d) in p.base_url_definitions.iter().enumerate() {
                tx.execute(
                    "INSERT INTO base_url_definitions \
                 (id, project_id, name, description, sort_order) \
                 VALUES (?,?,?,?,?)",
                    params![d.id, p.id, d.name, d.description, i as i64],
                )?;
            }
            // `baseUrl` is the legacy field on Project; we don't persist
            // it into its own column (the schema has none). The runtime
            // migrates legacy data into env.baseUrls[defId] at boot.
            let _ = p.base_url.as_ref();
        }

        // The fixture file keeps its three-section shape (modules /
        // collections / endpoints) because that is how a human reads a
        // sample tree. The tables those sections used to map onto are
        // gone, so this is where the translation happens: one `nodes`
        // row per entry, plus an `endpoint_details` row for each
        // endpoint.
        //
        // `root_id` is the one value the fixture does not carry — it was
        // implied by the module each collection hung off. Derived here
        // and memoised per owner so the lookup is a map read, not a
        // re-walk for every descendant.
        let mut owner: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

        for m in &fx.modules {
            insert_node(
                &tx,
                &ResourceNode {
                    id: m.id.clone(),
                    root_id: m.project_id.clone(),
                    // A module's parent is the project, not another node.
                    parent_id: None,
                    kind: ResourceNodeType::Module,
                    name: m.name.clone(),
                    description: m.description.clone(),
                    sort_order: m.sort_order,
                    created_at: fx.now,
                    updated_at: fx.now,
                },
            )?;
            owner.insert(&m.id, &m.project_id);
        }

        for c in &fx.collections {
            let root_id = owner.get(c.module_id.as_str()).copied().ok_or_else(|| {
                AppError::Validation(format!(
                    "fixture collection {} names unknown module {}",
                    c.id, c.module_id
                ))
            })?;
            // A nested collection's `parent_id` is the collection, not
            // the module — the type matrix allows either, and the fixture
            // says which by naming a `parentCollectionId`.
            let parent = c
                .parent_collection_id
                .as_deref()
                .unwrap_or(c.module_id.as_str());
            insert_node(
                &tx,
                &ResourceNode {
                    id: c.id.clone(),
                    root_id: root_id.to_string(),
                    parent_id: Some(parent.to_string()),
                    kind: ResourceNodeType::Collection,
                    name: c.name.clone(),
                    description: c.description.clone(),
                    sort_order: c.sort_order,
                    created_at: fx.now,
                    updated_at: fx.now,
                },
            )?;
            owner.insert(&c.id, root_id);
        }

        for e in &fx.endpoints {
            let root_id = owner
                .get(e.collection_id.as_str())
                .copied()
                .ok_or_else(|| {
                    AppError::Validation(format!(
                        "fixture endpoint {} names unknown collection {}",
                        e.id, e.collection_id
                    ))
                })?;
            insert_node(
                &tx,
                &ResourceNode {
                    id: e.id.clone(),
                    root_id: root_id.to_string(),
                    parent_id: Some(e.collection_id.clone()),
                    kind: ResourceNodeType::Endpoint,
                    name: e.name.clone(),
                    description: e.description.clone(),
                    sort_order: e.sort_order,
                    created_at: e.created_at,
                    updated_at: e.updated_at,
                },
            )?;
            // The node and its configuration land together: an endpoint
            // with no detail row is a tree the reader cannot open.
            tx.execute(
                "INSERT INTO endpoint_details \
             (node_id, method, url, environment_id, base_url_definition_id, \
              base_url_override, params_json, headers_json, body_json, \
              auth_json, docs, notes, tags_json) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    e.id,
                    e.method,
                    e.url,
                    e.environment_id,
                    e.base_url_definition_id,
                    e.base_url_override,
                    serde_json::to_string(&e.params)?,
                    serde_json::to_string(&e.headers)?,
                    serde_json::to_string(&e.body)?,
                    serde_json::to_string(&e.auth)?,
                    e.docs,
                    e.notes,
                    serde_json::to_string(&e.tags)?,
                ],
            )?;
        }

        for env in &fx.environments {
            let module_base_urls_json = serde_json::to_string(&env.module_base_urls)?;
            let variables_json = serde_json::to_string(&env.variables)?;
            tx.execute(
                "INSERT INTO environments \
             (id, project_id, name, is_active, module_base_urls_json, variables_json) \
             VALUES (?,?,?,?,?,?)",
                params![
                    env.id,
                    env.project_id,
                    env.name,
                    env.is_active as i64,
                    module_base_urls_json,
                    variables_json,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

// -------------------------------------------------------------------------
// Shared helper macros / functions for the CRUD modules.
// -------------------------------------------------------------------------

/// Re-export of `Arc<str>` for callers that want cheap-clone handles
/// to id strings. Not used yet but ready for the orchestration layer.
#[allow(dead_code)]
pub(crate) fn arc_str(s: impl Into<String>) -> Arc<str> {
    Arc::from(s.into())
}

/// Error type for a JSON-in-TEXT column that failed to decode.
///
/// `rusqlite`'s `FromSqlConversionFailure` wants
/// `Box<dyn StdError + Send + Sync + 'static>`, so the message is
/// parked in this tiny string-backed type.
#[derive(Debug)]
pub(crate) struct JsonColError(pub String);

impl std::fmt::Display for JsonColError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonColError {}

/// Map a serde failure on a JSON column to rusqlite's conversion error,
/// tagged with the column name.
///
/// Shaped this way so the rest of the row still loads and the caller
/// sees *which* column was malformed rather than an opaque decode
/// failure. Shared by every module that keeps structured data in a TEXT
/// column.
pub(crate) fn json_col_err(col: &str, e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(JsonColError(format!("{col}: {e}"))),
    )
}

// -------------------------------------------------------------------------
// Tests for the top-level surface: pool, init, migrate, ping, with_tx retry.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

/// Tiny per-process counter to give each test a unique temp dir.
/// Hoisted to its own module so test files don't fight over a global
/// mutable state in `mod tests`.
#[doc(hidden)]
pub mod test_counter {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    pub fn next() -> u64 {
        N.fetch_add(1, Ordering::Relaxed)
    }
}
