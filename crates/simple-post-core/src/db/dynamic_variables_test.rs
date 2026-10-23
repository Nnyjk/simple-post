use super::*;

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;
use crate::db;
use crate::db::nodes;
use crate::domain::{
    AuthConfig, DynamicVariable, DynamicVariablePatch, EndpointDetail, HttpMethod, Project,
    RequestBody, ResourceNode, ResourceNodeType, ValueMode,
};
use std::path::PathBuf;

/// 一个含一个项目、一个模块、一个 collection、两个 endpoint 的库。
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-dynamic-vars-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        nodes::create(c, &node("m1", ResourceNodeType::Module, None, 0), None)?;
        nodes::create(
            c,
            &node("c1", ResourceNodeType::Collection, Some("m1"), 0),
            None,
        )?;
        nodes::create(
            c,
            &node("e1", ResourceNodeType::Endpoint, Some("c1"), 0),
            Some(&detail("e1", "/consume")),
        )?;
        nodes::create(
            c,
            &node("e2", ResourceNodeType::Endpoint, Some("c1"), 1),
            Some(&detail("e2", "/other")),
        )?;
        Ok(())
    })
    .expect("seed tree");
    (pool, dir)
}

fn node(id: &str, kind: ResourceNodeType, parent: Option<&str>, sort: i64) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: "p1".into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: sort,
        created_at: 1,
        updated_at: 1,
    }
}

fn detail(node_id: &str, url: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Post,
        url: url.into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    }
}

fn definition(id: &str, name: &str) -> DynamicVariable {
    DynamicVariable {
        id: id.into(),
        project_id: "p1".into(),
        name: name.into(),
        description: None,
        source_endpoint_id: None,
        source_endpoint_name: None,
        source_inline_json: Some(INLINE_SOURCE.to_string()),
        extract_path: "$".into(),
        value_mode: ValueMode::Auto,
        ttl_seconds: 60,
        stale_fallback: false,
        enabled: true,
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    }
}

#[test]
fn a_definition_round_trips() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"), "p1")).expect("create");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.name, "sns");
    assert_eq!(got.project_id, "p1");
    assert_eq!(got.extract_path, "$");
    assert_eq!(got.value_mode, ValueMode::Auto);
    assert_eq!(got.ttl_seconds, 60);
    assert!(got.enabled);
}

#[test]
fn the_source_endpoint_name_is_joined_in() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    // 数据源二选一：引用接口时不能同时带内联请求。
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def, "p1")).expect("create");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.source_endpoint_id.as_deref(), Some("e2"));
    assert_eq!(got.source_endpoint_name.as_deref(), Some("e2"));
}

#[test]
fn two_definitions_in_one_project_cannot_share_a_name() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"), "p1")).expect("create");
    let err = db::with_tx(&pool, |c| create(c, &definition("d2", "sns"), "p1"))
        .expect_err("同名应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn an_illegal_name_is_refused() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| create(c, &definition("d1", "sns-list"), "p1"))
        .expect_err("带连字符的名字应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
    assert!(!is_valid_name("sns-list"));
    assert!(is_valid_name("sns_list2"));
}

#[test]
fn a_patch_leaves_absent_fields_alone() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"), "p1")).expect("create");
    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                ttl_seconds: Some(120),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.ttl_seconds, 120);
    assert_eq!(got.name, "sns");
    assert_eq!(got.value_mode, ValueMode::Auto);
}

#[test]
fn an_explicit_null_clears_the_description() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.description = Some("站点清单".into());
    db::with_tx(&pool, |c| create(c, &def, "p1")).expect("create");
    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                description: Some(None),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert!(got.description.is_none());
}

#[test]
fn deleting_the_source_endpoint_keeps_the_definition() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def, "p1")).expect("create");
    db::with_tx(&pool, |c| nodes::delete(c, "e2")).expect("delete endpoint");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert!(got.source_endpoint_id.is_none());
    assert_eq!(got.name, "sns");
}

#[test]
fn a_definition_without_a_source_is_refused() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_inline_json = None;
    let err = db::with_tx(&pool, |c| create(c, &def, "p1")).expect_err("没有数据源应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn a_definition_with_two_sources_is_refused() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    let err = db::with_tx(&pool, |c| create(c, &def, "p1")).expect_err("两个数据源应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn deleting_a_definition_removes_its_cache_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"), "p1")).expect("create");
    db::with_tx(&pool, |c| {
        crate::db::dynamic_variable_cache::upsert(
            c,
            &crate::domain::DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: "env1".into(),
                value_json: Some("[1]".into()),
                fetched_at: 1,
                expires_at: 2,
                is_override: false,
                override_expires_at: None,
            },
        )
    })
    .expect("cache");
    db::with_tx(&pool, |c| delete(c, "d1")).expect("delete");

    let rows: i64 = db::with_conn(&pool, |c| {
        c.query_row("SELECT COUNT(*) FROM dynamic_variable_cache", [], |row| {
            row.get(0)
        })
        .map_err(crate::error::AppError::Sqlite)
    })
    .expect("count");
    assert_eq!(rows, 0);
}

#[test]
fn the_endpoints_referencing_a_name_are_listed() {
    let (pool, _dir) = fresh_pool();
    // e1 的 body 里写 `{{sns}}`，e2 不写。
    db::with_tx(&pool, |c| {
        db::endpoint_details::update(
            c,
            "e1",
            crate::domain::EndpointDetailPatch {
                body: Some(RequestBody {
                    mode: crate::domain::BodyMode::Json,
                    content: "{\"sns\": {{sns}}}".into(),
                    schema: None,
                }),
                ..Default::default()
            },
        )
    })
    .expect("update detail");

    let refs = db::with_conn(&pool, |c| referencing_endpoints(c, "p1", "sns")).expect("scan");
    assert_eq!(refs, vec!["e1".to_string()]);

    let none = db::with_conn(&pool, |c| referencing_endpoints(c, "p1", "other")).expect("scan");
    assert!(none.is_empty());
}

#[test]
fn an_unknown_definition_is_not_found() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_conn(&pool, |c| get(c, "nope")).expect_err("未知定义");
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}
