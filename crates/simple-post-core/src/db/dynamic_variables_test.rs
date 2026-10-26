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
        name: name.into(),
        description: None,
        source_endpoint_id: None,
        source_endpoint_name: None,
        source_inline_json: Some(INLINE_SOURCE.to_string()),
        source_environment_id: None,
        source_base_url_definition_id: None,
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
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.name, "sns");
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
    db::with_tx(&pool, |c| create(c, &def)).expect("create");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.source_endpoint_id.as_deref(), Some("e2"));
    assert_eq!(got.source_endpoint_name.as_deref(), Some("e2"));
}

#[test]
fn two_definitions_cannot_share_a_name() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
    let err =
        db::with_tx(&pool, |c| create(c, &definition("d2", "sns"))).expect_err("同名应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn an_illegal_name_is_refused() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| create(c, &definition("d1", "sns-list")))
        .expect_err("带连字符的名字应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
    assert!(!is_valid_name("sns-list"));
    assert!(is_valid_name("sns_list2"));
}

#[test]
fn a_patch_leaves_absent_fields_alone() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
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
    db::with_tx(&pool, |c| create(c, &def)).expect("create");
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
    db::with_tx(&pool, |c| create(c, &def)).expect("create");
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
    let err = db::with_tx(&pool, |c| create(c, &def)).expect_err("没有数据源应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn a_definition_with_two_sources_is_refused() {
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    let err = db::with_tx(&pool, |c| create(c, &def)).expect_err("两个数据源应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn deleting_a_definition_removes_its_cache_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");
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

    let map = db::with_conn(&pool, |c| {
        referencing_endpoints(c, &["sns".to_string(), "other".to_string()])
    })
    .expect("scan");
    let refs = map.get("sns").expect("sns 的引用");
    let ids: Vec<String> = refs.iter().map(|r| r.endpoint_id.clone()).collect();
    assert_eq!(ids, vec!["e1".to_string()]);
    assert_eq!(refs[0].project_id, "p1");
    assert_eq!(refs[0].project_name, "First");

    assert!(map.get("other").expect("other 的引用").is_empty());
}

#[test]
fn one_scan_answers_every_name() {
    let (pool, _dir) = fresh_pool();
    // e1 引用 `{{sns}}`，e2 引用 `{{token}}`。
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
    .expect("update e1");
    db::with_tx(&pool, |c| {
        db::endpoint_details::update(
            c,
            "e2",
            crate::domain::EndpointDetailPatch {
                body: Some(RequestBody {
                    mode: crate::domain::BodyMode::Json,
                    content: "{\"token\": {{token}}}".into(),
                    schema: None,
                }),
                ..Default::default()
            },
        )
    })
    .expect("update e2");

    // 一次遍历同时回答两个名字：端点各自只落到自己引用的那个名字下。
    let map = db::with_conn(&pool, |c| {
        referencing_endpoints(c, &["sns".to_string(), "token".to_string()])
    })
    .expect("scan");
    let sns_ids: Vec<String> = map
        .get("sns")
        .expect("sns 的引用")
        .iter()
        .map(|r| r.endpoint_id.clone())
        .collect();
    assert_eq!(sns_ids, vec!["e1".to_string()]);
    let token_ids: Vec<String> = map
        .get("token")
        .expect("token 的引用")
        .iter()
        .map(|r| r.endpoint_id.clone())
        .collect();
    assert_eq!(token_ids, vec!["e2".to_string()]);
}

#[test]
fn an_empty_name_list_scans_nothing() {
    let (pool, _dir) = fresh_pool();
    let map = db::with_conn(&pool, |c| referencing_endpoints(c, &[])).expect("scan");
    assert!(map.is_empty());
}

#[test]
fn an_unknown_definition_is_not_found() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_conn(&pool, |c| get(c, "nope")).expect_err("未知定义");
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}

/// 另一个项目，以及它的一个接口。
fn seed_another_project(pool: &db::DbPool) {
    db::with_tx(pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p2".into(),
                name: "Second".into(),
                description: None,
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        nodes::create(
            c,
            &ResourceNode {
                root_id: "p2".into(),
                ..node("m2", ResourceNodeType::Module, None, 0)
            },
            None,
        )?;
        nodes::create(
            c,
            &ResourceNode {
                root_id: "p2".into(),
                ..node("e9", ResourceNodeType::Endpoint, Some("m2"), 0)
            },
            Some(&detail("e9", "/other-project")),
        )?;
        Ok(())
    })
    .expect("seed second project");
}

#[test]
fn a_data_source_endpoint_from_another_project_is_allowed() {
    let (pool, _dir) = fresh_pool();
    seed_another_project(&pool);

    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e9".into());
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def)).expect("跨项目的数据源是允许的");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.source_endpoint_id.as_deref(), Some("e9"));

    // 权限范围落在数据源接口所属的项目上。
    let scope = db::with_conn(&pool, |c| source_project_id_of(c, "d1")).expect("scope");
    assert_eq!(scope.as_deref(), Some("p2"));
}

#[test]
fn an_inline_source_has_no_project_scope() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");

    let scope = db::with_conn(&pool, |c| source_project_id_of(c, "d1")).expect("scope");
    assert_eq!(scope, None);
}

#[test]
fn pointing_an_existing_definition_at_another_projects_endpoint_is_allowed() {
    let (pool, _dir) = fresh_pool();
    seed_another_project(&pool);
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                source_endpoint_id: Some(Some("e9".into())),
                source_inline_json: Some(None),
                ..Default::default()
            },
        )
    })
    .expect("改到跨项目的数据源");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.source_endpoint_id.as_deref(), Some("e9"));
    assert!(got.source_inline_json.is_none());
}

#[test]
fn an_environment_from_another_project_is_refused() {
    let (pool, _dir) = fresh_pool();
    seed_another_project(&pool);
    db::with_tx(&pool, |c| {
        db::environments::create(
            c,
            &crate::domain::Environment {
                id: "env-p2".into(),
                project_id: "p2".into(),
                name: "prod".into(),
                is_active: false,
                base_urls: Default::default(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: vec![],
            },
        )
    })
    .expect("seed env");

    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    def.source_inline_json = None;
    def.source_environment_id = Some("env-p2".into());
    let err = db::with_tx(&pool, |c| create(c, &def)).expect_err("别的项目的环境应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn a_definition_whose_source_endpoint_was_deleted_can_still_be_renamed() {
    // 外键把 `source_endpoint_id` 置成 NULL，故两个来源字段都空了——那是「接口被删了」，
    // 不是「这条定义没有数据源」。这种定义仍应允许改别的字段，否则用户改个名就会收到
    // 一句「必须有一个数据源」，而真正的原因（接口被删了）被这句话盖住了。
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def)).expect("create");
    db::with_tx(&pool, |c| nodes::delete(c, "e2")).expect("delete endpoint");

    db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                name: Some("points".into()),
                ..Default::default()
            },
        )
    })
    .expect("改名不该被「必须有一个数据源」挡住");

    let got = db::with_conn(&pool, |c| get(c, "d1")).expect("read");
    assert_eq!(got.name, "points");
    assert!(got.source_endpoint_id.is_none());
}

#[test]
fn clearing_both_sources_explicitly_is_still_refused() {
    // 上面那条放的是「改别的字段」；把两个来源字段**都**清空是另一回事：那是用户
    // 亲手把定义写成没有数据源的状态，故仍要拒。
    let (pool, _dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e2".into());
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def)).expect("create");

    let err = db::with_tx(&pool, |c| {
        update(
            c,
            "d1",
            DynamicVariablePatch {
                source_endpoint_id: Some(None),
                source_inline_json: Some(None),
                ..Default::default()
            },
        )
    })
    .expect_err("两个来源都被清空应该被拒");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn a_database_failure_is_not_reported_as_a_definition_without_a_project() {
    // MCP 工具靠 `source_project_id_of` 的返回值决定要不要过按项目的权限门，故「数据库
    // 坏了」不能被读成「这条定义没有数据源项目」——那等于把门关掉。
    let (pool, dir) = fresh_pool();
    let mut def = definition("d1", "sns");
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;
    db::with_tx(&pool, |c| create(c, &def)).expect("create");

    // 删掉 nodes 表，制造一次真正的数据库错误。
    {
        let conn = rusqlite::Connection::open(dir.join("store.db")).expect("open");
        conn.execute_batch("PRAGMA foreign_keys = OFF; DROP TABLE nodes;")
            .expect("drop nodes");
    }

    let mut conn = rusqlite::Connection::open(dir.join("store.db")).expect("open");
    let err = source_project_id_of(&mut conn, "d1")
        .expect_err("数据库出错不该被读成「这条定义没有数据源项目」");
    assert!(matches!(err, crate::error::AppError::Sqlite(_)), "{err:?}");
}

#[test]
fn a_duplicate_id_is_reported_as_an_id_conflict() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| create(c, &definition("d1", "sns"))).expect("create");

    // id 重复但名字不重复：真正的原因是主键冲突，故不能说成「重名」。
    let err =
        db::with_tx(&pool, |c| create(c, &definition("d1", "other"))).expect_err("id 重复应该被拒");
    let message = match err {
        crate::error::AppError::Validation(message) => message,
        other => panic!("期望一条校验错误，实际是 {other:?}"),
    };
    assert!(message.contains("id"), "实际消息是：{message}");
    assert!(!message.contains("已有一个叫"), "不该报成重名：{message}");
}
