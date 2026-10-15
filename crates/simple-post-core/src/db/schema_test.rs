// schema 版本。

use std::path::Path;

use rusqlite::Connection;

use super::schema::remove_db_files;
use super::test_helpers::{fresh_empty_pool, fresh_pool, temp_dir};
use super::*;
use crate::error::AppError;

/// 种一个形状像版本号之前布局的 `store.db`——三表树、给定的 `user_version`，再加一行，
/// 使重建可从外部观察。
/// `legacy_marker` 是当前 `schema.sql` 绝不会创建的表。它是与阶段无关的证据，证明文件是被
/// **替换**而非就地清空的：断言「新 schema 恰好定义了哪些表」会随布局演进不断变化，而且
/// 即便重建悄悄复用了旧文件也照样会通过。
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
    // 在此处 drop，好让句柄在 `init` 试图删除文件之前释放——Windows 上打开的句柄会让删除失败。
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

    // 旧的 `legacy_marker` 表彻底消失。这就是「文件被替换」的检查：就地清空旧文件会把它留下，
    // 而一个本构建已不再理解的 schema，是它必须永远继续理解下去的东西。
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

    // 重新打开一个本构建已经写过的库，必须不碰它。
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
    // 没有 -wal，没有 -shm。别处的打开失败就可能留下这种状态。
    remove_db_files(&dir.join("store.db")).expect("missing sidecar is not an error");
    assert!(!dir.join("store.db").exists());
}
