use super::*;

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;
use crate::db;
use crate::domain::{DynamicCacheEntry, DynamicVariable, ValueMode};

fn entry(definition_id: &str, environment_id: &str) -> DynamicCacheEntry {
    DynamicCacheEntry {
        definition_id: definition_id.into(),
        environment_id: environment_id.into(),
        value_json: Some("[{\"code\":\"A001\"}]".into()),
        fetched_at: 1_000,
        expires_at: 2_000,
        is_override: false,
        override_expires_at: None,
    }
}

/// 一个含一个项目与一条动态变量定义的库。缓存行有外键指向定义，故先有定义。
fn fresh_pool() -> (db::DbPool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-dynamic-cache-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &crate::domain::Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        for (id, name) in [("d1", "sns"), ("d2", "factors")] {
            db::dynamic_variables::create(
                c,
                &DynamicVariable {
                    id: id.into(),
                    project_id: Some("p1".into()),
                    name: name.into(),
                    description: None,
                    source_endpoint_id: None,
                    source_endpoint_name: None,
                    source_inline_json: Some(INLINE_SOURCE.to_string()),
                    source_environment_id: None,
                    extract_path: "$".into(),
                    transform_script: None,
                    value_mode: ValueMode::Auto,
                    ttl_seconds: 60,
                    stale_fallback: false,
                    enabled: true,
                    sort_order: 0,
                    created_at: 1,
                    updated_at: 1,
                },
            )?;
        }
        Ok(())
    })
    .expect("seed");
    (pool, dir)
}

#[test]
fn an_absent_row_is_not_an_error() {
    let (pool, _dir) = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "d1", "env1")).expect("read");
    assert!(got.is_none());
}

#[test]
fn a_row_round_trips() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env1"))).expect("write");
    let got = db::with_conn(&pool, |c| get(c, "d1", "env1")).expect("read");
    let got = got.expect("有一行");
    assert_eq!(got.definition_id, "d1");
    assert_eq!(got.environment_id, "env1");
    assert_eq!(got.value_json.as_deref(), Some("[{\"code\":\"A001\"}]"));
    assert_eq!(got.fetched_at, 1_000);
    assert_eq!(got.expires_at, 2_000);
    assert!(!got.is_override);
}

#[test]
fn the_cache_key_is_the_definition_and_the_environment() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env1"))).expect("write");
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env2"))).expect("write");

    let one = db::with_conn(&pool, |c| get(c, "d1", "env1")).expect("read");
    let two = db::with_conn(&pool, |c| get(c, "d1", "env2")).expect("read");
    assert!(one.is_some());
    assert!(two.is_some());

    let none_env = db::with_conn(&pool, |c| get(c, "d1", NO_ENVIRONMENT)).expect("read");
    assert!(none_env.is_none());
}

#[test]
fn writing_twice_replaces_the_row_instead_of_adding_one() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env1"))).expect("write");
    let mut second = entry("d1", "env1");
    second.value_json = Some("[]".into());
    second.is_override = true;
    db::with_tx(&pool, |c| upsert(c, &second)).expect("write");

    let rows: i64 = db::with_conn(&pool, |c| {
        c.query_row("SELECT COUNT(*) FROM dynamic_variable_cache", [], |row| {
            row.get(0)
        })
        .map_err(crate::error::AppError::Sqlite)
    })
    .expect("count");
    assert_eq!(rows, 1);

    let got = db::with_conn(&pool, |c| get(c, "d1", "env1")).expect("read");
    let got = got.expect("有一行");
    assert_eq!(got.value_json.as_deref(), Some("[]"));
    assert!(got.is_override);
}

#[test]
fn deleting_a_row_removes_it() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env1"))).expect("write");
    db::with_tx(&pool, |c| delete(c, "d1", "env1")).expect("delete");
    let got = db::with_conn(&pool, |c| get(c, "d1", "env1")).expect("read");
    assert!(got.is_none());
}

#[test]
fn the_rows_of_one_definition_are_listed_together() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env1"))).expect("write");
    db::with_tx(&pool, |c| upsert(c, &entry("d1", "env2"))).expect("write");
    db::with_tx(&pool, |c| upsert(c, &entry("d2", "env1"))).expect("write");

    let rows = db::with_conn(&pool, |c| list_by_definition(c, "d1")).expect("read");
    assert_eq!(rows.len(), 2);
}
