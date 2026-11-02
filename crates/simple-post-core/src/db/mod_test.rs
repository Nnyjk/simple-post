use super::*;
use crate::error::AppError;

use super::test_helpers::{fresh_empty_pool, fresh_pool};

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

    // `ping` 会真正走一次连接。
    ping(&pool).expect("ping");
}

#[test]
fn init_leaves_a_fresh_database_empty() {
    // Simple Post 不带任何种子数据：`db::init` 建完表即止。行只来自用户或 MCP 客户端。
    let (pool, _dir) = fresh_empty_pool();
    for table in ["projects", "nodes", "endpoint_details", "environments"] {
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
    // 同一个库上第二次调用：每个 CREATE 都是 IF NOT EXISTS，故重跑 schema 是无操作且行不受影响。
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
    // 在闭包里制造错误；回滚应把行数恢复到 3（seed 出来的项目数）。
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
