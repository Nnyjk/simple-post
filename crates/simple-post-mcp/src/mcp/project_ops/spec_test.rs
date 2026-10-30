//! `spec_upsert` 的测试：一次调用落一份完整规格，以及过门行为。
//!
//! 这一组守三件事：
//!
//! 1. **一次调用真的落全了** —— 模块、集合、环境、端点一个都不少。
//! 2. **过门在写之前发生** —— 项目的权限不够时，库里一个字节都不动。
//! 3. **响应带着「还缺什么」** —— agent 不必先发一次请求才知道少了 base URL。

use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::mcp::AppState;
use simple_post_core::db;
use simple_post_core::error::AppError;
use simple_post_core::permissions::Permission;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-spec-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 建一个项目，并把它的权限设成 `Full` —— 写工具要求的就是这一档。
fn project_with_full_permission(pool: &db::DbPool, project_id: &str, name: &str) {
    db::with_tx(pool, |c| {
        db::projects::create(
            c,
            &simple_post_core::domain::Project {
                id: project_id.to_string(),
                name: name.to_string(),
                description: None,
                color: "#3B82F6".to_string(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        db::permissions::set(c, project_id, Permission::Full)?;
        Ok(())
    })
    .unwrap();
}

/// 把工具响应里的那段 JSON 取出来。
fn payload(res: &crate::mcp::CallToolResult) -> serde_json::Value {
    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    serde_json::from_str(text).expect("工具响应应当是 JSON")
}

async fn call(
    state: &Arc<AppState>,
    spec: serde_json::Value,
) -> Result<crate::mcp::CallToolResult, AppError> {
    crate::mcp::project_ops::spec::tool_spec_upsert(state, spec).await
}

// =========================================================================
// 一次调用落全一份规格
// =========================================================================

#[tokio::test]
async fn one_call_lands_the_module_the_collections_the_environment_and_the_endpoint() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");

    let res = call(
        &st,
        json!({
            "project_id": "p_spec",
            "module": "交易",
            "collection_path": "订单/明细",
            "environment": {
                "name": "dev",
                "variables": { "token": "t-1" },
                "base_url": "https://dev.example.com",
                "activate": true
            },
            "endpoint": {
                "name": "查询订单",
                "method": "POST",
                "url": "/api/orders/query",
                "params": [{ "key": "page", "value": "1" }],
                "headers": [{ "key": "X-Trace", "value": "1" }],
                "body": {
                    "mode": "json",
                    "content": "{\"orderNo\":\"{{orderNo}}\"}",
                    "schema": "{\"type\":\"object\",\"properties\":{\"orderNo\":{\"type\":\"string\"}}}"
                },
                "auth": { "kind": "bearer", "bearer": "{{token}}" },
                "tags": ["订单"],
                "pre_request_script": "pm.environment.set('a','b');",
                "test_script": "pm.test('ok', () => pm.response.code === 200);"
            }
        }),
    )
    .await
    .expect("spec_upsert 应当成功");

    let out = payload(&res);

    // 模块与集合。
    assert_eq!(out["module"]["name"], "交易");
    assert_eq!(out["module"]["created"], true);
    assert_eq!(out["collections"].as_array().map(Vec::len), Some(2));
    assert_eq!(out["collections"][1]["name"], "明细");

    // 环境。
    assert_eq!(out["environment"]["name"], "dev");
    assert_eq!(out["environment"]["activated"], true);

    // 端点：本体与创建标志都在。
    assert_eq!(out["endpoint"]["created"], true);
    assert_eq!(out["endpoint"]["updated"], false);
    assert_eq!(out["endpoint"]["detail"]["method"], "POST");
    assert_eq!(out["endpoint"]["detail"]["url"], "/api/orders/query");

    // 库里真的有了那些行。
    let endpoint_id = out["endpoint"]["id"].as_str().expect("端点 id");
    db::with_conn(&st.pool, |c| {
        let detail = db::endpoint_details::get(c, endpoint_id)?;
        assert_eq!(detail.params.len(), 1);
        assert_eq!(detail.headers.len(), 1);
        assert_eq!(detail.tags, vec!["订单".to_string()]);
        assert!(detail.pre_request_script.is_some());
        assert!(detail.test_script.is_some());
        assert_eq!(detail.auth.kind, simple_post_core::domain::AuthType::Bearer);
        assert!(detail.body.schema.is_some());
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn the_response_says_what_is_still_missing() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");

    // 这一份规格没有 base URL：端点发出去时没有 host。
    let res = call(
        &st,
        json!({
            "project_id": "p_spec",
            "module": "交易",
            "endpoint": { "name": "查询订单", "method": "GET", "url": "/api/orders" }
        }),
    )
    .await
    .expect("spec_upsert 应当成功");

    let out = payload(&res);
    let missing = out["missing"].as_array().expect("missing 是一个数组");
    assert!(
        !missing.is_empty(),
        "没有 base URL 时，响应里要点名缺什么：{out}"
    );
    let text = missing
        .iter()
        .map(|m| m.as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(text.contains("base URL"), "{text}");
}

#[tokio::test]
async fn applying_the_same_spec_twice_changes_nothing_the_second_time() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");
    let spec = json!({
        "project_id": "p_spec",
        "module": "交易",
        "collection_path": "订单",
        "endpoint": { "name": "查询订单", "method": "GET", "url": "/api/orders" }
    });

    let first = payload(&call(&st, spec.clone()).await.expect("第一次应当成功"));
    let second = payload(&call(&st, spec).await.expect("第二次应当成功"));

    assert_eq!(first["module"]["created"], true);
    assert_eq!(second["module"]["created"], false);
    assert_eq!(first["endpoint"]["created"], true);
    assert_eq!(second["endpoint"]["created"], false);
    assert_eq!(first["endpoint"]["id"], second["endpoint"]["id"]);
}

// =========================================================================
// 过门
// =========================================================================

#[tokio::test]
async fn the_permission_gate_refuses_a_project_that_is_not_writable() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");
    // 另一个项目只给到 `ReadCall` —— 写工具要求 `Full`。
    db::with_tx(&st.pool, |c| {
        db::projects::create(
            c,
            &simple_post_core::domain::Project {
                id: "p_ro".to_string(),
                name: "只读项目".to_string(),
                description: None,
                color: "#3B82F6".to_string(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        db::permissions::set(c, "p_ro", Permission::ReadCall)?;
        Ok(())
    })
    .unwrap();

    let res = call(
        &st,
        json!({
            "project_id": "p_ro",
            "module": "交易",
            "endpoint": { "name": "查询订单", "method": "GET", "url": "/api/orders" }
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::PermissionDenied(_))),
        "权限不够时应当拒绝：{res:?}"
    );

    // 拒绝之后库里一个字节都不动。
    let count = db::with_conn(&st.pool, |c| {
        Ok(db::nodes::list_by_root(c, "p_ro").unwrap().len())
    })
    .unwrap();
    assert_eq!(count, 0, "被拒绝的调用不该建出任何节点");
}

#[tokio::test]
async fn a_project_without_a_permission_row_is_refused() {
    // 权限行缺失时按项目那扇门给的是 `ReadCall`，低于写工具要求的 `Full`。
    let st = state();
    db::with_tx(&st.pool, |c| {
        db::projects::create(
            c,
            &simple_post_core::domain::Project {
                id: "p_default".to_string(),
                name: "默认权限项目".to_string(),
                description: None,
                color: "#3B82F6".to_string(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        Ok(())
    })
    .unwrap();

    let res = call(
        &st,
        json!({
            "project_id": "p_default",
            "module": "交易",
            "endpoint": { "name": "查询订单", "method": "GET", "url": "/api/orders" }
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::PermissionDenied(_))),
        "没有权限行时应当拒绝：{res:?}"
    );
}

// =========================================================================
// 输入校验
// =========================================================================

#[tokio::test]
async fn an_unknown_field_in_the_spec_is_refused_by_name() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");
    let res = call(
        &st,
        json!({
            "project_id": "p_spec",
            "module": "交易",
            "typo_field": 1,
            "endpoint": { "name": "查询订单", "method": "GET", "url": "/api/orders" }
        }),
    )
    .await;
    let e = res.expect_err("不认识的字段应当报错");
    assert!(e.to_string().contains("typo_field"), "{e}");
}

#[tokio::test]
async fn a_payload_that_looks_like_a_schema_is_refused() {
    let st = state();
    project_with_full_permission(&st.pool, "p_spec", "规格项目");
    let res = call(
        &st,
        json!({
            "project_id": "p_spec",
            "module": "交易",
            "endpoint": {
                "name": "查询订单",
                "method": "POST",
                "url": "/api/orders",
                "body": {
                    "mode": "json",
                    "content": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}"
                }
            }
        }),
    )
    .await;
    let e = res.expect_err("载荷与定义写反了应当报错");
    assert!(e.to_string().contains("body_schema"), "{e}");
}
