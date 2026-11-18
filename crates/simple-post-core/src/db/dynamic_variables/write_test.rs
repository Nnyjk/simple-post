//! `update` 的缓存失效：定义内容变了，按旧定义算出来的缓存行随之作废。

use super::*;

use crate::db;
use crate::domain::ValueMode;
use std::path::PathBuf;

/// 一份最小可用的内联数据源。
const INLINE_SOURCE: &str = "{\"method\":\"GET\",\"url\":\"http://127.0.0.1:9/points\"}";

/// 一个含一个项目的空库。
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-dynamic-var-write-test-{}-{}",
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
        Ok(())
    })
    .expect("seed project");
    (pool, dir)
}

fn definition(id: &str, name: &str) -> DynamicVariable {
    DynamicVariable {
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
    }
}

/// 给 `d1` 放一行自动取回的缓存。
fn seed_cache(pool: &db::DbPool, environment_id: &str) {
    seed_row(pool, environment_id, false);
}

/// 给 `d1` 放一行用户钉住的覆盖值。
fn seed_override(pool: &db::DbPool, environment_id: &str) {
    seed_row(pool, environment_id, true);
}

fn seed_row(pool: &db::DbPool, environment_id: &str, is_override: bool) {
    db::with_tx(pool, |c| {
        crate::db::dynamic_variable_cache::upsert(
            c,
            &crate::domain::DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: environment_id.into(),
                value_json: Some("[1]".into()),
                fetched_at: 1,
                expires_at: 100_000,
                is_override,
                override_expires_at: None,
            },
        )
    })
    .expect("cache");
}

/// 一条定义还在的缓存行：环境 id 与是否覆盖值，按环境 id 排序。
fn cache_rows(pool: &db::DbPool, definition_id: &str) -> Vec<(String, bool)> {
    db::with_conn(pool, |c| {
        let mut stmt = c
            .prepare(
                "SELECT environment_id, is_override FROM dynamic_variable_cache \
                 WHERE definition_id = ?1 ORDER BY environment_id",
            )
            .map_err(AppError::Sqlite)?;
        let rows = stmt
            .query_map(params![definition_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            })
            .map_err(AppError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::Sqlite)?;
        Ok(rows)
    })
    .expect("read cache rows")
}

#[test]
fn changing_the_extraction_rule_drops_the_auto_fetched_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");
    seed_cache(&pool, "env1");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                extract_path: Some("$[0].name".into()),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert!(
        cache_rows(&pool, "d1").is_empty(),
        "按旧抽取规则取到的值全部作废"
    );
}

#[test]
fn resubmitting_the_same_extraction_rule_keeps_the_cache_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");

    // 表单在编辑态会把整份草稿回传，未改动的字段也带着原值。
    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                extract_path: Some("$".into()),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert_eq!(cache_rows(&pool, "d1"), vec![("".to_string(), false)]);
}

#[test]
fn renaming_keeps_the_cache_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                name: Some("sites".into()),
                description: Some(Some("站点清单".into())),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert_eq!(cache_rows(&pool, "d1"), vec![("".to_string(), false)]);
}

#[test]
fn changing_the_ttl_keeps_the_cache_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                ttl_seconds: Some(3600),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert_eq!(cache_rows(&pool, "d1"), vec![("".to_string(), false)]);
}

#[test]
fn an_override_value_survives_an_extraction_rule_change() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");
    seed_override(&pool, "env1");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                extract_path: Some("$[0].name".into()),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert_eq!(
        cache_rows(&pool, "d1"),
        vec![("env1".to_string(), true)],
        "覆盖值不由定义推导，定义怎么改都留着"
    );
}

#[test]
fn changing_the_inline_source_drops_the_auto_fetched_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    seed_cache(&pool, "");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                source_inline_json: Some(Some(
                    r#"{"method":"GET","url":"http://127.0.0.1:9/others"}"#.to_string(),
                )),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    assert!(
        cache_rows(&pool, "d1").is_empty(),
        "换了数据源，旧值不再成立"
    );
}
