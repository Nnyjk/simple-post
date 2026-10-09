use super::*;
use std::path::PathBuf;

/// Build a fresh pool rooted at a temp directory, with the bundled
/// fixture set loaded. Each test owns its own dir so they're
/// independent. Returns the dir so the caller can `keep` / `drop`
/// it as desired.
///
/// `db::init` no longer seeds — see [`fresh_empty_pool`] for the
/// un-seeded shape. Tests below assert against `p_demo` and friends, so
/// they opt into the fixtures explicitly.
fn fresh_pool() -> (DbPool, PathBuf) {
    let (pool, dir) = fresh_empty_pool();
    test_support::seed(&pool);
    (pool, dir)
}

/// Same as [`fresh_pool`] but **no rows inserted** — the state a real
/// first boot leaves behind. Use this when the test is about migration
/// or about how the app behaves on an empty database.
fn fresh_empty_pool() -> (DbPool, PathBuf) {
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
fn init_leaves_a_fresh_database_empty() {
    // Simple Post ships no seed data: `db::init` creates tables and
    // stops. Rows only ever come from the user or from an MCP client.
    let (pool, _dir) = fresh_empty_pool();
    for table in [
        "projects",
        "nodes",
        "endpoint_details",
        "environments",
        "base_url_definitions",
    ] {
        let count: i64 = with_conn(&pool, |c| {
            Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
        })
        .unwrap();
        assert_eq!(count, 0, "{table} should be empty after init");
    }
}

#[test]
fn migrate_is_idempotent() {
    let (pool, _dir) = fresh_pool();
    // Second call on the same DB: every CREATE is IF NOT EXISTS, so
    // re-running the schema is a no-op and rows are untouched.
    with_conn(&pool, migrate).expect("second migrate");
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(count, 3, "migrate must not duplicate or drop rows");
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
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
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
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
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

// -------------------------------------------------------------------------
// Schema versioning.
// -------------------------------------------------------------------------

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-schema-test-{}-{}-{}",
        tag,
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Plant a `store.db` shaped like the pre-versioning layout — the
/// three-table tree, the given `user_version`, and one row in it so a
/// rebuild is observable from the outside.
///
/// `legacy_marker` is a table the current `schema.sql` would never
/// create. It is the phase-independent witness that the file was
/// *replaced* rather than emptied in place: an assertion about which
/// tables the new schema happens to define would keep changing as the
/// layout moves, and would pass even if the rebuild quietly reused the
/// old file.
fn plant_legacy_store(dir: &Path, version: i64) {
    std::fs::create_dir_all(dir).unwrap();
    let conn = Connection::open(dir.join("store.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, \
             color TEXT, created_at INTEGER, updated_at INTEGER);
         CREATE TABLE modules (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, \
             name TEXT NOT NULL, description TEXT, sort_order INTEGER NOT NULL);
         CREATE TABLE legacy_marker (note TEXT);
         INSERT INTO projects VALUES ('p_legacy', 'Legacy', '#000', 1, 1);
         INSERT INTO modules VALUES ('m_legacy', 'p_legacy', 'Legacy Module', NULL, 0);
         INSERT INTO legacy_marker VALUES ('this build must never write a table like this');",
    )
    .unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
    // Dropped here so the handle is released before `init` tries to
    // delete the file — on Windows an open handle makes removal fail.
}

#[test]
fn schema_version_matches_the_constant() {
    let (pool, _dir) = fresh_empty_pool();
    let v: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(
        v, SCHEMA_VERSION,
        "the PRAGMA at the bottom of schema.sql must agree with SCHEMA_VERSION; \
         bump the constant when the layout changes shape"
    );
}

#[test]
fn init_rebuilds_a_legacy_store() {
    let dir = temp_dir("legacy");
    plant_legacy_store(&dir, 0);

    let pool = init(&dir).expect("init");

    let projects: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects, 0, "legacy rows must not survive a rebuild");

    // The legacy `legacy_marker` table is gone entirely. This is the
    // "the file was replaced" check: emptying the old file in place
    // would have left it behind, and a schema this build no longer
    // understands is a schema it has to keep understanding forever.
    let still_there: i64 = with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='legacy_marker'",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(
        still_there, 0,
        "rebuild must replace the file, not just clear its rows"
    );

    let v: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(v, SCHEMA_VERSION, "the rebuilt store is stamped current");
}

#[test]
fn init_keeps_a_current_store_intact() {
    let (pool, dir) = fresh_pool();
    assert_eq!(
        with_conn(&pool, |c| Ok(c.query_row(
            "SELECT COUNT(*) FROM projects",
            [],
            |r| r.get::<_, i64>(0)
        )?))
        .unwrap(),
        3
    );
    drop(pool);

    // Re-opening a store this build already wrote must not touch it.
    let pool = init(&dir).expect("second init");
    let projects: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects, 3, "a current store survives re-init intact");
}

#[test]
fn init_refuses_a_database_from_a_newer_build() {
    let dir = temp_dir("newer");
    plant_legacy_store(&dir, SCHEMA_VERSION + 1);

    let err = init(&dir).expect_err("a newer schema must not be clobbered");
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation, got {err:?}"
    );
    assert!(
        dir.join("store.db").exists(),
        "a downgrade must never delete data it does not understand"
    );
}

#[test]
fn remove_db_files_takes_the_wal_sidecars_with_it() {
    let dir = temp_dir("sidecars");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["store.db", "store.db-wal", "store.db-shm"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    remove_db_files(&dir.join("store.db")).unwrap();

    for name in ["store.db", "store.db-wal", "store.db-shm"] {
        assert!(
            !dir.join(name).exists(),
            "{name} must be gone — a surviving store.db-wal would be replayed \
             into the freshly created store.db"
        );
    }
}

#[test]
fn remove_db_files_tolerates_absent_sidecars() {
    let dir = temp_dir("no-sidecar");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("store.db"), b"x").unwrap();
    // No -wal, no -shm. A failed open elsewhere can leave it that way.
    remove_db_files(&dir.join("store.db")).expect("missing sidecar is not an error");
    assert!(!dir.join("store.db").exists());
}
