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
        "modules",
        "collections",
        "endpoints",
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
