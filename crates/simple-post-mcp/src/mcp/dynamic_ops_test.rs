//! 动态变量 MCP 工具的测试。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use super::super::{
    dispatch, registered_tool_names, required_capability, AppState, CallToolResult,
};
use simple_post_core::db;
use simple_post_core::domain::Permission;
use simple_post_core::permissions::Capability;

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 一个含一个项目的测试库。项目的 agent 权限给 `Full`，故写工具可用。
///
/// 全局能力档位默认是 `read_only`（缺行即如此），故这里显式写成 `full_access`——
/// 否则连 `dynamic_var_create` 都会被能力门挡下，而那与本文件要验的东西无关。
fn test_state(permission: Permission) -> Arc<AppState> {
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
        db::projects::create(
            c,
            &simple_post_core::domain::Project {
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
        db::permissions::set(c, "p1", permission)?;
        db::preferences::set_mcp_policy(
            c,
            simple_post_core::permissions::CapabilityPolicy::FullAccess,
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
            "project_id": "p1",
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

    let listed = dispatch(&state, "dynamic_var_list", json!({ "project_id": "p1" }))
        .await
        .expect("列出");
    let listed_text = text_of(listed);
    assert!(listed_text.contains("sns"), "{listed_text}");
    assert!(listed_text.contains("120"), "{listed_text}");
}

#[tokio::test]
async fn an_illegal_name_is_refused() {
    let state = test_state(Permission::Full);
    let err = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "project_id": "p1", "name": "sns-list" }),
    )
    .await
    .expect_err("带连字符的名字应该被拒");
    assert!(err.to_string().contains("validation"), "{err}");
}

#[tokio::test]
async fn a_read_only_project_cannot_create_a_definition() {
    let state = test_state(Permission::Read);
    let err = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "project_id": "p1", "name": "sns", "source_inline_json": INLINE_SOURCE }),
    )
    .await
    .expect_err("只读项目不该能写");
    assert!(err.to_string().contains("permission denied"), "{err}");
}

#[tokio::test]
async fn updating_and_deleting_a_definition_work() {
    let state = test_state(Permission::Full);
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({ "project_id": "p1", "name": "sns", "source_inline_json": INLINE_SOURCE }),
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
    let listed = dispatch(&state, "dynamic_var_list", json!({ "project_id": "p1" }))
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
        json!({ "project_id": "p1", "name": "sns", "source_inline_json": INLINE_SOURCE }),
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
async fn a_resolve_with_a_broken_source_reports_the_reason() {
    let state = test_state(Permission::Full);
    // 数据源是一份**不是 JSON** 的内联请求：取数器解析它时失败，故报 `source_error`，
    // 且不必真的发网络请求。
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "project_id": "p1",
            "name": "sns",
            "source_inline_json": "not json"
        }),
    )
    .await
    .expect("创建");

    let resolved = dispatch(
        &state,
        "dynamic_var_resolve",
        json!({ "project_id": "p1", "names": ["sns"] }),
    )
    .await
    .expect("解析");
    let text = text_of(resolved);
    assert!(text.contains("source_error"), "{text}");
}
