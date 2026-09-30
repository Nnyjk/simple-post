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
use rusqlite::{Connection, OpenFlags};

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
/// see [`ensure_current_schema`]. When that happens the rebuild is
/// recorded under `preferences["db.recreated_at"]` so the UI can tell
/// the user their previous data is gone instead of silently rendering
/// an empty tree.
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
        with_tx(&pool, |c| {
            preferences::set(
                c,
                preferences::keys::DB_RECREATED_AT,
                &crate::domain::now_ms(),
            )
        })?;
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

/// Run the embedded `schema.sql`. Idempotent — safe to call on every
/// boot.
///
/// Every `CREATE` in the schema is `IF NOT EXISTS`, so re-running against
/// an already-migrated database is a no-op. **No rows are inserted**:
/// a fresh install starts empty and is populated by the user or by an
/// MCP client. Test-only seed data lives in `test_support`.
pub fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    Ok(conn.execute_batch(SCHEMA_SQL)?)
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
        #[serde(default)]
        base_urls: serde_json::Value,
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
            let base_urls_json = serde_json::to_string(&env.base_urls)?;
            let variables_json = serde_json::to_string(&env.variables)?;
            tx.execute(
                "INSERT INTO environments \
             (id, project_id, name, is_active, base_urls_json, variables_json) \
             VALUES (?,?,?,?,?,?)",
                params![
                    env.id,
                    env.project_id,
                    env.name,
                    env.is_active as i64,
                    base_urls_json,
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
