// schema 版本。

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

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

/// 种一个**旧形状**的动态变量表：定义属于项目，故表里有 `project_id` 列。
///
/// 这是动态变量还是项目级资产时写下的形状。它与当前形状同名同表，`CREATE TABLE IF NOT
/// EXISTS` 对它是空操作——故 `migrate` 必须自己认出它并丢掉重建。
fn plant_project_scoped_dynamic_variables(pool: &DbPool) {
    with_tx(pool, |c| {
        c.execute_batch(
            "DROP TABLE dynamic_variable_cache;
             DROP TABLE dynamic_variables;
             CREATE TABLE dynamic_variables (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 description TEXT,
                 source_endpoint_id TEXT REFERENCES nodes(id) ON DELETE SET NULL,
                 source_inline_json TEXT,
                 extract_path TEXT NOT NULL DEFAULT '$',
                 value_mode TEXT NOT NULL DEFAULT 'auto',
                 ttl_seconds INTEGER NOT NULL DEFAULT 60,
                 stale_fallback INTEGER NOT NULL DEFAULT 0,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 sort_order INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE UNIQUE INDEX idx_dynamic_vars_name ON dynamic_variables(project_id, name);
             INSERT INTO dynamic_variables
                 (id, project_id, name, extract_path, value_mode, ttl_seconds,
                  stale_fallback, enabled, sort_order, created_at, updated_at)
             VALUES ('d_old', 'p_demo', 'sns', '$', 'auto', 60, 0, 1, 0, 1, 1);",
        )
        .map_err(AppError::Sqlite)
    })
    .expect("plant the old shape");
}

/// 一张表里有没有某一列。
fn column_count(pool: &DbPool, table: &str, column: &str) -> i64 {
    with_conn(pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, column],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

#[test]
fn the_project_scoped_dynamic_variable_tables_are_replaced_by_the_global_ones() {
    let (pool, dir) = fresh_pool();
    let projects_before: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    plant_project_scoped_dynamic_variables(&pool);
    drop(pool);

    // 重新打开同一个库。`SCHEMA_VERSION` 没变，故 `ensure_current_schema` 不删它；
    // `migrate` 认出旧形状并把两张动态变量表丢掉重建。
    let pool = init(&dir).expect("second init");

    let old_rows: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM dynamic_variables", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(old_rows, 0, "旧形状的定义应当被丢掉");

    assert_eq!(
        column_count(&pool, "dynamic_variables", "project_id"),
        0,
        "重建后的表不该再有 project_id 列"
    );
    assert_eq!(
        column_count(&pool, "dynamic_variables", "source_environment_id"),
        1,
        "重建后的表应当有取数环境那一列"
    );

    // 其余数据不动：丢的只是那两张表。
    let projects_after: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects_after, projects_before, "项目不该被这次迁移牵连");
}

/// 种一张建了却没人用过的 `request_settings` 表，并放一行进去。
///
/// 它是单例行（`CHECK id=1`），三列是超时 / 跟随重定向 / 响应体积上限。请求旋钮的现役
/// 容器是 `preferences` 表里的同名**行**（`db::preferences::keys::REQUEST_SETTINGS`），
/// 故这张表从建立起就没有读者，也从来没有一行被写进去过。
fn plant_obsolete_request_settings_table(pool: &DbPool) {
    with_tx(pool, |c| {
        c.execute_batch(
            "CREATE TABLE request_settings (
                 id INTEGER PRIMARY KEY CHECK(id=1),
                 timeout_ms INTEGER NOT NULL DEFAULT 30000,
                 follow_redirects INTEGER NOT NULL DEFAULT 1,
                 max_response_size_kb INTEGER NOT NULL DEFAULT 5120
             );
             INSERT INTO request_settings VALUES (1, 12345, 0, 999);",
        )
        .map_err(AppError::Sqlite)
    })
    .expect("plant the obsolete table");
}

/// 库里有几张叫这个名字的表。
fn table_count(pool: &DbPool, name: &str) -> i64 {
    with_conn(pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

/// `preferences` 表里某个键的原始值。键不在时是 `None`。
fn preference_value(pool: &DbPool, key: &str) -> Option<String> {
    with_conn(pool, |c| {
        Ok(c.query_row(
            "SELECT value FROM preferences WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
    })
    .unwrap()
}

#[test]
fn a_fresh_store_has_no_request_settings_table() {
    let (pool, _dir) = fresh_empty_pool();

    assert_eq!(
        table_count(&pool, "request_settings"),
        0,
        "新库不该再建这张没有人读的表"
    );
    // 现役容器得在：`preferences` 表是设置旋钮住的地方。
    assert_eq!(
        table_count(&pool, "preferences"),
        1,
        "`preferences` 表是设置旋钮的现役容器，不该被牵连"
    );
}

#[test]
fn the_obsolete_request_settings_table_is_dropped_on_open() {
    let (pool, dir) = fresh_pool();
    let projects_before: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    plant_obsolete_request_settings_table(&pool);
    drop(pool);

    // 重新打开同一个库。`SCHEMA_VERSION` 没变，故 `ensure_current_schema` 不删它；
    // `migrate` 把那张没有人读的表丢掉。
    let pool = init(&dir).expect("second init");

    assert_eq!(
        table_count(&pool, "request_settings"),
        0,
        "残留表应当被清除"
    );

    // 其余数据不动：丢的只是那一张表。
    let projects_after: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects_after, projects_before, "项目不该被这次清理牵连");
}

#[test]
fn the_request_settings_preference_row_survives_the_cleanup() {
    let (pool, dir) = fresh_empty_pool();
    let written = crate::domain::RequestSettings {
        timeout_ms: 5_000,
        follow_redirects: false,
        max_response_size_kb: 2_048,
    };
    with_tx(&pool, |c| {
        crate::db::preferences::set_request_settings(c, &written)
    })
    .expect("write the preference row");
    plant_obsolete_request_settings_table(&pool);
    drop(pool);

    let pool = init(&dir).expect("second init");

    // 表名与行名同名，故这条断言是必要的：清理那张**表**不能连**行**一起清掉。
    let read_back = with_conn(&pool, crate::db::preferences::request_settings).unwrap();
    assert_eq!(
        read_back.timeout_ms, 5_000,
        "设置旋钮的行不该被这次清理牵连"
    );
    assert!(
        !read_back.follow_redirects,
        "设置旋钮的行不该被这次清理牵连"
    );
    assert_eq!(
        read_back.max_response_size_kb, 2_048,
        "设置旋钮的行不该被这次清理牵连"
    );
}

#[test]
fn the_other_preference_keys_survive_the_cleanup() {
    let (pool, dir) = fresh_empty_pool();
    let seeded = [
        ("mcp.auto_start", "true"),
        ("mcp.policy", "\"full_access\""),
        ("mcp.http_config", "{\"host\":\"127.0.0.1\",\"port\":4173}"),
    ];
    with_tx(&pool, |c| {
        for (key, value) in seeded {
            c.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(AppError::Sqlite)?;
        }
        Ok(())
    })
    .expect("seed the preference rows");
    plant_obsolete_request_settings_table(&pool);
    drop(pool);

    let pool = init(&dir).expect("second init");

    // 丢的是一张**表**，不是 `preferences` 表本身——那几个键一个字都不该变。
    for (key, value) in seeded {
        assert_eq!(
            preference_value(&pool, key).as_deref(),
            Some(value),
            "`{key}` 不该被这次清理牵连"
        );
    }
}

#[test]
fn the_cleanup_is_a_no_op_when_the_table_is_already_gone() {
    // 另一个进程可能已经删掉了它：GUI、守护进程与 `serve-web` 各自打开同一个
    // `store.db`，第二个打开者看到的就是一张已经没有那张表的库。那一步清理必须是
    // 空操作，不能让 `migrate` 失败——`db::init` 失败会让 GUI 与守护进程直接退出。
    let dir = temp_dir("already-gone");
    let pool = init(&dir).expect("init");
    plant_obsolete_request_settings_table(&pool);
    with_tx(&pool, |c| {
        c.execute_batch("DROP TABLE request_settings;")
            .map_err(AppError::Sqlite)
    })
    .expect("another opener dropped it");
    drop(pool);

    let pool = init(&dir).expect("second init");

    assert_eq!(
        table_count(&pool, "request_settings"),
        0,
        "表已经不在，清理必须是空操作"
    );
}

#[test]
fn a_second_opener_sees_a_store_it_can_still_migrate() {
    let dir = temp_dir("second-opener");
    let pool = init(&dir).expect("first init");
    plant_obsolete_request_settings_table(&pool);
    drop(pool);

    // 第一个打开者清掉它。
    let pool = init(&dir).expect("second init");
    assert_eq!(
        table_count(&pool, "request_settings"),
        0,
        "残留表应当被清除"
    );
    drop(pool);

    // 第二个打开者看到的是一张已经没有那张表的库，同样不该失败。
    let pool = init(&dir).expect("third init");
    assert_eq!(
        table_count(&pool, "request_settings"),
        0,
        "残留表应当被清除"
    );
}
