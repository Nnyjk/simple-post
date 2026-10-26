//! 动态变量 MCP 工具的测试。
//!
//! 定义是**全局**的，故这批测试不再按项目分桶：创建与列出都不带项目参数，权限门落在
//! 「数据源接口所属的项目」上。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use super::super::{
    dispatch, registered_tool_names, required_capability, AppState, CallToolResult,
};
use simple_post_core::db;
use simple_post_core::domain::{
    AuthConfig, BodyMode, EndpointDetail, Environment, HttpMethod, Permission, Project,
    RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::permissions::Capability;

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 一个含两个项目的测试库。每个项目下各有一个模块与一个接口，p1 另有一个环境。
///
/// 两个项目的 agent 权限给同一档位，故写工具是否可用由测试自己决定。
///
/// 全局能力档位默认是 `read_only`（缺行即如此），故这里显式写成 `full_access`——
/// 否则连 `dynamic_var_create` 都会被能力门挡下，而那与本文件要验的东西无关。
fn test_state(permission: Permission) -> Arc<AppState> {
    test_state_pair(permission, permission)
}

/// 两个项目各给各的权限档位。
///
/// 权限门落在数据源接口所属的项目上，故只有当两个项目档位不同时，才能验出
/// 「只能写其中一个项目的数据源」这类按项目区分的行为。
fn test_state_pair(p1_permission: Permission, p2_permission: Permission) -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-dynamic-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = simple_post_core::db::init(&dir).unwrap();
    db::with_tx(&pool, |c| {
        for (project_id, project_name) in [("p1", "First"), ("p2", "Second")] {
            db::projects::create(
                c,
                &Project {
                    id: project_id.into(),
                    name: project_name.into(),
                    description: None,
                    default_base_url_definition_id: None,
                    base_url_definitions: vec![],
                    base_url: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )?;
        }
        db::permissions::set(c, "p1", p1_permission)?;
        db::permissions::set(c, "p2", p2_permission)?;
        db::preferences::set_mcp_policy(
            c,
            simple_post_core::permissions::CapabilityPolicy::FullAccess,
        )?;

        // p1 的树：一个模块下一个接口。
        db::nodes::create(
            c,
            &ResourceNode {
                id: "m1".into(),
                root_id: "p1".into(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "模块".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            None,
        )?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e1".into(),
                root_id: "p1".into(),
                parent_id: Some("m1".into()),
                kind: ResourceNodeType::Endpoint,
                name: "取数接口".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            Some(&EndpointDetail {
                node_id: "e1".into(),
                method: HttpMethod::Get,
                url: "/points".into(),
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
            }),
        )?;
        db::environments::create(
            c,
            &Environment {
                id: "env1".into(),
                project_id: "p1".into(),
                name: "dev".into(),
                is_active: false,
                base_urls: Default::default(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: vec![],
            },
        )?;
        // p2 的树：一个模块下一个接口。改指数据源到 p2 的接口时，落点就是它。
        db::nodes::create(
            c,
            &ResourceNode {
                id: "m2".into(),
                root_id: "p2".into(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "另一个模块".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            None,
        )?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e2".into(),
                root_id: "p2".into(),
                parent_id: Some("m2".into()),
                kind: ResourceNodeType::Endpoint,
                name: "另一个项目的取数接口".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            Some(&EndpointDetail {
                node_id: "e2".into(),
                method: HttpMethod::Get,
                url: "/points".into(),
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
            }),
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(AppState {
        pool,
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 工具返回的文本内容。
fn text_of(result: CallToolResult) -> String {
    let content = serde_json::to_value(&result).expect("序列化");
    content["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// 列出工具返回的数组。
fn listed_of(result: CallToolResult) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&text_of(result))
        .expect("json")
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_seven_tools_are_registered_with_a_capability_row() {
    let names = registered_tool_names();
    for expected in [
        "dynamic_var_list",
        "dynamic_var_get",
        "dynamic_var_create",
        "dynamic_var_update",
        "dynamic_var_delete",
        "dynamic_var_resolve",
        "dynamic_var_set_override",
    ] {
        assert!(names.iter().any(|n| n == expected), "缺少工具 {expected}");
        assert!(
            required_capability(expected).is_some(),
            "{expected} 缺 REQUIRED_CAPABILITY 行"
        );
    }
}

#[test]
fn the_resolve_tool_demands_the_send_capability() {
    assert_eq!(
        required_capability("dynamic_var_resolve"),
        Some(Capability::Send)
    );
    assert_eq!(
        required_capability("dynamic_var_list"),
        Some(Capability::Read)
    );
}

#[tokio::test]
async fn creating_and_listing_a_definition_round_trips() {
    let state = test_state(Permission::Full);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "name": "sns",
            "extract_path": "$.list",
            "source_inline_json": INLINE_SOURCE,
            "ttl_seconds": 120,
        }),
    )
    .await
    .expect("创建");
    let created_text = text_of(created);
    assert!(created_text.contains("sns"), "{created_text}");

    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let listed_text = text_of(listed);
    assert!(listed_text.contains("sns"), "{listed_text}");
    assert!(listed_text.contains("120"), "{listed_text}");
}

#[tokio::test]
async fn a_definition_is_not_belonged_to_a_project() {
    // 定义里没有项目这一层：创建时不给项目，列出来时也不带项目。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");

    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let entries = listed_of(listed);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].get("projectId").is_none(), "{:?}", entries[0]);
}

#[tokio::test]
async fn a_definition_in_one_project_is_visible_from_another() {
    // 全局作用域的意义：一条定义写一次，任何项目的端点都能引用它。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");

    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let entries = listed_of(listed);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["name"], "sns");
}

#[tokio::test]
async fn an_illegal_name_is_refused() {
    let state = test_state(Permission::Full);
    let err = dispatch(&state, "dynamic_var_create", json!({ "name": "sns-list" }))
        .await
        .expect_err("带连字符的名字应该被拒");
    assert!(err.to_string().contains("validation"), "{err}");
}

#[tokio::test]
async fn a_read_only_project_cannot_create_a_definition_over_its_own_endpoint() {
    // 权限门落在数据源接口所属的项目上：数据源接口在只读项目里，创建就该被拒。
    let state = test_state(Permission::Read);
    let err = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_endpoint_id": "e1" }),
    )
    .await
    .expect_err("只读项目不该能写");
    assert!(err.to_string().contains("permission denied"), "{err}");
}

#[tokio::test]
async fn an_inline_source_has_no_project_to_gate_on() {
    // 内联请求没有项目可落，故只由全局能力档位裁决：只读的项目挡不住它。
    let state = test_state(Permission::Read);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("内联数据源没有项目可落");
}

#[tokio::test]
async fn updating_and_deleting_a_definition_work() {
    let state = test_state(Permission::Full);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");
    let id = serde_json::from_str::<serde_json::Value>(&text_of(created)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let updated = dispatch(
        &state,
        "dynamic_var_update",
        json!({
            "definition_id": id,
            "patch": { "name": "factors", "ttlSeconds": 30 }
        }),
    )
    .await
    .expect("改");
    let updated_text = text_of(updated);
    assert!(updated_text.contains("factors"), "{updated_text}");
    assert!(updated_text.contains("30"), "{updated_text}");

    dispatch(&state, "dynamic_var_delete", json!({ "definition_id": id }))
        .await
        .expect("删");
    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    assert!(!text_of(listed).contains("factors"));
}

#[tokio::test]
async fn an_override_is_written_and_cleared() {
    let state = test_state(Permission::Full);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");
    let id = serde_json::from_str::<serde_json::Value>(&text_of(created)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let set = dispatch(
        &state,
        "dynamic_var_set_override",
        json!({ "definition_id": id, "value": "[{\"code\":\"A001\"}]" }),
    )
    .await
    .expect("写覆盖值");
    let set_text = text_of(set);
    assert!(set_text.contains("A001"), "{set_text}");
    assert!(set_text.contains("override"), "{set_text}");

    let cleared = dispatch(
        &state,
        "dynamic_var_set_override",
        json!({ "definition_id": id, "value": null }),
    )
    .await
    .expect("清覆盖值");
    let cleared_text = text_of(cleared);
    assert!(!cleared_text.contains("A001"), "{cleared_text}");
}

#[tokio::test]
async fn an_override_of_an_inline_definition_lands_on_the_key_the_resolve_reads() {
    // 写与读必须落在同一个环境键上。内联数据源没有环境，那个键就是空串——
    // 否则手动指定的值落在别处，解析看不见它。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "name": "sns",
            "source_inline_json": INLINE_SOURCE
        }),
    )
    .await
    .expect("创建");
    let created = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let listed = serde_json::from_str::<serde_json::Value>(&text_of(created)).expect("json");
    let id = listed[0]["id"].as_str().expect("id").to_string();

    crate::dynamic::ops::set_override(
        &state.pool,
        &id,
        Some("[{\"code\":\"Z999\"}]".to_string()),
        None,
        true,
    )
    .expect("写覆盖值");

    let resolution = crate::dynamic::ops::resolve(&state.pool, &["sns".to_string()], false)
        .await
        .expect("解析");
    assert_eq!(
        resolution.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"Z999\"}]"),
        "解析应当拿到手动指定的值"
    );
}

#[tokio::test]
async fn an_override_lands_on_the_definitions_own_environment() {
    // 数据源接口在 p1，p1 有一个环境 env1。取数环境由定义自己推导，故覆盖值落在
    // (定义 × env1) 那一格上，解析从同一格读它。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "name": "sns",
            "source_endpoint_id": "e1"
        }),
    )
    .await
    .expect("创建");
    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let entries = listed_of(listed);
    assert_eq!(entries[0]["effectiveEnvironmentId"], "env1");
    let id = entries[0]["id"].as_str().expect("id").to_string();

    crate::dynamic::ops::set_override(
        &state.pool,
        &id,
        Some("[{\"code\":\"Z999\"}]".to_string()),
        None,
        true,
    )
    .expect("写覆盖值");

    let resolution = crate::dynamic::ops::resolve(&state.pool, &["sns".to_string()], false)
        .await
        .expect("解析");
    assert_eq!(
        resolution.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"Z999\"}]"),
        "解析应当拿到手动指定的值"
    );
}

#[tokio::test]
async fn a_resolve_with_a_broken_source_reports_the_reason() {
    let state = test_state(Permission::Full);
    // 数据源是一份**不是 JSON** 的内联请求：取数器解析它时失败，故报 `source_error`，
    // 且不必真的发网络请求。
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "name": "sns",
            "source_inline_json": "not json"
        }),
    )
    .await
    .expect("创建");

    let resolved = dispatch(&state, "dynamic_var_resolve", json!({ "names": ["sns"] }))
        .await
        .expect("解析");
    let text = text_of(resolved);
    assert!(text.contains("source_error"), "{text}");
}

#[tokio::test]
async fn a_refresh_really_fetches_the_value() {
    // 刷新用**定义自己的**取数上下文：调用方不传项目，故它不可能取错地方。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "name": "sns",
            "source_inline_json": "not json"
        }),
    )
    .await
    .expect("创建");
    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let id = serde_json::from_str::<serde_json::Value>(&text_of(listed)).expect("json")[0]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let status = crate::dynamic::ops::refresh(&state.pool, &id)
        .await
        .expect("刷新");
    assert_eq!(status.name, "sns");
    assert!(
        status.error.is_some(),
        "应当真的取过一次数（内联数据源不是合法 JSON，故有错）"
    );
}

#[tokio::test]
async fn pointing_a_definition_at_another_projects_endpoint_checks_that_project_too() {
    // 只查改之前的数据源项目，等于给「把数据源搬到别的项目」留了一条不经门的路：
    // 改完之后取数会用到新项目的 base URL 与凭据，故新项目也得查。
    let state = test_state_pair(Permission::Full, Permission::Read);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");
    let id = serde_json::from_str::<serde_json::Value>(&text_of(created)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let err = dispatch(
        &state,
        "dynamic_var_update",
        json!({ "definition_id": id, "patch": { "sourceEndpointId": "e2" } }),
    )
    .await
    .expect_err("只读项目不该能成为数据源");
    assert!(err.to_string().contains("permission denied"), "{err}");
}

#[tokio::test]
async fn a_definition_whose_source_endpoint_was_deleted_can_still_be_renamed() {
    // 「两个来源字段都空」= 数据源接口被删了，那是**既有状态**，不是一次非法写入。
    // 故改个名字不该被当成「这条定义没有数据源」而拒掉。
    let state = test_state(Permission::Full);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_endpoint_id": "e1" }),
    )
    .await
    .expect("创建");
    let id = serde_json::from_str::<serde_json::Value>(&text_of(created)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 删掉数据源接口：外键的 ON DELETE SET NULL 把两个来源列一起清空。
    db::with_conn(&state.pool, |c| db::nodes::delete(c, "e1")).expect("删掉数据源接口");

    let updated = dispatch(
        &state,
        "dynamic_var_update",
        json!({ "definition_id": id, "patch": { "name": "factors" } }),
    )
    .await
    .expect("改名");
    assert!(text_of(updated).contains("factors"));
}

#[tokio::test]
async fn the_list_tool_does_not_carry_the_referencing_endpoints() {
    // 「哪些端点引用了它」是**别的项目的**接口名与 id。`dynamic_var_list` 没有按项目的门
    // （定义是全局的，没有单一归属行可查），带上它就成了一条枚举所有项目接口名的旁路，
    // 而 `endpoint_list` / `endpoint_search` 恰恰拒绝无项目地扫全部项目。
    let state = test_state(Permission::Full);
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect("创建");

    // p2 里放一个引用了 {{sns}} 的接口：REST 那条路会把它列出来，MCP 这条路不列。
    db::with_tx(&state.pool, |c| {
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e3".into(),
                root_id: "p2".into(),
                parent_id: Some("m2".into()),
                kind: ResourceNodeType::Endpoint,
                name: "引用了变量的接口".into(),
                description: None,
                sort_order: 1,
                created_at: 1,
                updated_at: 1,
            },
            Some(&EndpointDetail {
                node_id: "e3".into(),
                method: HttpMethod::Post,
                url: "/orders".into(),
                environment_id: None,
                base_url_definition_id: None,
                base_url_override: None,
                params: vec![],
                headers: vec![],
                body: RequestBody {
                    mode: BodyMode::Json,
                    content: r#"{"q":"{{sns}}"}"#.to_string(),
                    schema: None,
                },
                auth: AuthConfig::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
            }),
        )
    })
    .expect("建接口");

    let listed = dispatch(&state, "dynamic_var_list", json!({}))
        .await
        .expect("列出");
    let entries = listed_of(listed);
    let references = entries[0]["referencedBy"]
        .as_array()
        .expect("referencedBy 数组");
    assert!(
        references.is_empty(),
        "MCP 的列表不该带上别的项目的接口名：{references:?}"
    );
}
