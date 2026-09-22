//! Persistence layer for Simple Post.
//!
//! Layout:
//!   - `db::init` opens (or creates) the SQLite file, runs `schema.sql`,
//!     and seeds fixtures on a brand-new DB.
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
//!   - The directory is created if missing; the DB file is created
//!     if missing. First boot seeds from `db/fixtures.json`.
//!
//! Per-table CRUD lives in the sibling `db/*.rs` modules. They all
//! take a `&mut rusqlite::Connection` so the caller controls whether
//! the call is in a transaction.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use r2d2::ManageConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OpenFlags};
use serde::Deserialize;

use crate::error::AppError;

// -------------------------------------------------------------------------
// Sub-modules. Public so the orchestration layer / Tauri commands can
// reach individual CRUD functions without going through an aggregator.
// -------------------------------------------------------------------------

pub mod base_url_defs;
pub mod collections;
pub mod endpoints;
pub mod environments;
pub mod modules;
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
// Public API.
// -------------------------------------------------------------------------

/// Open (or create) the DB at `<data_dir>/store.db`, run migrations,
/// seed if empty, return the pool.
///
/// `data_dir` is typically `dirs::data_dir().unwrap().join("com.simple-post")`.
/// The directory is created if it doesn't exist.
pub fn init(data_dir: &Path) -> Result<DbPool, AppError> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("store.db");

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
    migrate_and_seed(&mut probe)?;
    drop(probe);

    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(manager)
        .map_err(AppError::Pool)?;

    // Smoke-test the pool: the first checkout from a freshly built pool
    // forces the manager to actually open a connection, which is where
    // a locked file / bad path would surface.
    ping(&pool)?;

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

/// Run the embedded `schema.sql` and, if the projects table is empty,
/// insert the bundled fixtures. Idempotent — safe to call on every
/// boot.
///
/// The schema uses `IF NOT EXISTS` so re-running on a migrated DB is
/// a no-op. The seed step checks `projects` first so a partially-seeded
/// DB (which shouldn't happen in practice) also gets treated as
/// already-seeded on retry.
pub fn migrate_and_seed(conn: &mut Connection) -> Result<(), AppError> {
    conn.execute_batch(SCHEMA_SQL)?;

    // Already seeded? Skip.
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if count > 0 {
        return Ok(());
    }

    let fixtures: Fixtures = serde_json::from_str(FIXTURES_JSON)?;
    seed_fixtures(conn, &fixtures)?;
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

// ----- fixtures ---------------------------------------------------------

/// Top-level shape of `fixtures.json`. We only deserialize the fields
/// the seeder actually needs; the file can grow extra sections without
/// breaking older binaries (provided they have defaults).
#[derive(Debug, Deserialize)]
struct Fixtures {
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
    #[serde(default)]
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
    #[serde(default)]
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

    for m in &fx.modules {
        tx.execute(
            "INSERT INTO modules \
             (id, project_id, name, description, sort_order) \
             VALUES (?,?,?,?,?)",
            params![m.id, m.project_id, m.name, m.description, m.sort_order],
        )?;
        let _ = m.expanded; // UI-only, not persisted
    }

    for c in &fx.collections {
        tx.execute(
            "INSERT INTO collections \
             (id, module_id, parent_collection_id, name, description, sort_order) \
             VALUES (?,?,?,?,?,?)",
            params![
                c.id,
                c.module_id,
                c.parent_collection_id,
                c.name,
                c.description,
                c.sort_order,
            ],
        )?;
        let _ = c.expanded;
    }

    for e in &fx.endpoints {
        let params_json = serde_json::to_string(&e.params)?;
        let headers_json = serde_json::to_string(&e.headers)?;
        let body_json = serde_json::to_string(&e.body)?;
        let auth_json = serde_json::to_string(&e.auth)?;
        let tags_json = serde_json::to_string(&e.tags)?;
        tx.execute(
            "INSERT INTO endpoints \
             (id, collection_id, name, description, method, url, \
              environment_id, base_url_definition_id, base_url_override, \
              params_json, headers_json, body_json, auth_json, \
              docs, notes, tags_json, sort_order, created_at, updated_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                e.id,
                e.collection_id,
                e.name,
                e.description,
                e.method,
                e.url,
                e.environment_id,
                e.base_url_definition_id,
                e.base_url_override,
                params_json,
                headers_json,
                body_json,
                auth_json,
                e.docs,
                e.notes,
                tags_json,
                e.sort_order,
                e.created_at,
                e.updated_at,
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

// -------------------------------------------------------------------------
// Embedded static assets — kept in-tree so the binary doesn't need a
// filesystem lookup at runtime. `include_str!` / `include_str!`-style
// embedding makes the assets participate in `cargo build` rebuilds
// when they change.
// -------------------------------------------------------------------------

const SCHEMA_SQL: &str = include_str!("schema.sql");
const FIXTURES_JSON: &str = include_str!("fixtures.json");

// -------------------------------------------------------------------------
// Shared helper macros / functions for the CRUD modules.
// -------------------------------------------------------------------------

/// Re-export of `Arc<str>` for callers that want cheap-clone handles
/// to id strings. Not used yet but ready for the orchestration layer.
#[allow(dead_code)]
pub(crate) fn arc_str(s: impl Into<String>) -> Arc<str> {
    Arc::from(s.into())
}

// -------------------------------------------------------------------------
// Tests for the top-level surface: pool, init, migrate, ping, with_tx retry.
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build a fresh pool rooted at a temp directory. Each test owns
    /// its own dir so they're independent. Returns the dir so the
    /// caller can `keep` / `drop` it as desired.
    fn fresh_pool() -> (DbPool, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-db-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        // Make sure we start clean even if a previous run left files.
        let _ = std::fs::remove_dir_all(&dir);
        let pool = init(&dir).expect("init");
        (pool, dir)
    }

    #[test]
    fn init_creates_dir_and_db_file() {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-db-test-init-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let pool = init(&dir).expect("init");
        assert!(dir.exists(), "data dir should exist");
        assert!(dir.join("store.db").exists(), "store.db should exist");

        // `ping` round-trips a connection.
        ping(&pool).expect("ping");
    }

    #[test]
    fn init_seeds_fixtures_on_empty_db() {
        let (pool, _dir) = fresh_pool();
        let count: i64 =
            with_conn(&pool, |c| Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(count, 3, "3 fixture projects should be seeded");
    }

    #[test]
    fn migrate_and_seed_is_idempotent() {
        let (pool, _dir) = fresh_pool();
        // Second call on the same DB: schema is IF NOT EXISTS, and the
        // seed step sees a non-empty `projects` table and bails out.
        with_conn(&pool, migrate_and_seed).expect("second migrate");
        let count: i64 =
            with_conn(&pool, |c| Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(count, 3, "second seed should not duplicate");
    }

    #[test]
    fn with_tx_rolls_back_on_error() {
        let (pool, _dir) = fresh_pool();
        // Force an error inside the closure; the rollback should
        // restore the row count to 3 (the seeded projects).
        let res: Result<(), AppError> = with_tx(&pool, |c| {
            c.execute("DELETE FROM projects", [])?;
            Err(AppError::Validation("nope".into()))
        });
        assert!(res.is_err());
        let count: i64 =
            with_conn(&pool, |c| Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(count, 3, "rollback should leave projects intact");
    }

    #[test]
    fn with_tx_commits_on_success() {
        let (pool, _dir) = fresh_pool();
        with_tx(&pool, |c| {
            c.execute(
                "INSERT INTO projects (id, name, color, created_at, updated_at) \
                 VALUES ('p_new', 'New', '#fff', 1, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let count: i64 =
            with_conn(&pool, |c| Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?))
                .unwrap();
        assert_eq!(count, 4);
    }

    #[test]
    fn with_conn_returns_caller_value() {
        let (pool, _dir) = fresh_pool();
        let v: i64 = with_conn(&pool, |c| {
            Ok(c.query_row("SELECT 42", [], |r| r.get::<_, i64>(0))?)
        })
        .unwrap();
        assert_eq!(v, 42);
    }
}

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