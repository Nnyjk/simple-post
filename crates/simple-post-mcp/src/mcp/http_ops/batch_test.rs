//! `api_run_collection` 的报告算术与失败分类。
//!
//! 这组断言盯两件事：
//!
//! 1. 报告的恒等式：总数 = 状态码分布之和 + 没拿到状态码 + 压根没发出去 + 未执行。
//!    分类逻辑住在 `batch.rs` 的 `Counts::add`。
//! 2. **压根没发出去**与**发出去了没响应**分得开。前者是准备阶段失败（缺请求配置、
//!    请求解析不出来），后者是传输层失败。两者都没有状态码，故只能靠判定的先后把它们
//!    分开。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use super::batch::{tool_api_run_collection, Counts};
use crate::mcp::{AppState, CallToolContent, CallToolResult};
use simple_post_core::db;
use simple_post_core::domain::{
    EndpointDetail, HttpMethod, Permission, Project, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

/// 一条结论的三要素：成功与否、状态码、失败分类。
///
/// `failure` 的取值与 `batch.rs` 里一致：`blocked`（压根没发出去）、`http`
/// （拿到了响应但不是 2xx）、`transport`（发出去了但没拿到响应）。
fn counts_of(outcomes: &[(bool, u16, Option<&str>)]) -> Counts {
    let mut counts = Counts::default();
    for (success, status, failure) in outcomes {
        counts.add(*success, *status, *failure);
    }
    counts
}

#[test]
fn three_classes_are_mutually_exclusive_and_exhaustive() {
    let outcomes = [
        (true, 200, None),
        (true, 201, None),
        (false, 404, Some("http")),
        (false, 500, Some("http")),
        (false, 0, Some("transport")),
        (false, 0, Some("blocked")),
    ];
    let counts = counts_of(&outcomes);

    // 三类之和恒等于结论条数 —— 少算一条就会与总数对不上。
    assert_eq!(counts.classified(), outcomes.len());
    assert_eq!(counts.success, 2);
    assert_eq!(counts.failed, 4);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.no_response, 1);
    assert_eq!(counts.status_counts.get(&200), Some(&1));
    assert_eq!(counts.status_counts.get(&201), Some(&1));
    assert_eq!(counts.status_counts.get(&404), Some(&1));
    assert_eq!(counts.status_counts.get(&500), Some(&1));
}

#[test]
fn the_report_identity_holds() {
    let outcomes = [
        (true, 200, None),
        (false, 500, Some("http")),
        (false, 0, Some("transport")),
        (false, 0, Some("blocked")),
    ];
    let counts = counts_of(&outcomes);
    let total = outcomes.len();

    // MCP 的调用是同步的，没有中止协议，故每一条都轮到过，`not_run` 恒为 0。
    let not_run = 0;
    assert_eq!(
        total,
        counts.status_counts.values().sum::<usize>()
            + counts.no_response
            + counts.blocked
            + not_run
    );
}

#[test]
fn blocked_is_not_counted_as_no_response() {
    // 「压根没发出去」与「没拿到响应」是两回事：前者是这条没准备好，后者是发出去了
    // 但对方没应答。两者都没有状态码，故必须靠判定的先后把它们分开。
    let counts = counts_of(&[(false, 0, Some("blocked"))]);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.no_response, 0);
    assert!(counts.status_counts.is_empty());
}

#[test]
fn a_status_code_never_lands_in_no_response() {
    let counts = counts_of(&[(false, 404, Some("http"))]);
    assert_eq!(counts.no_response, 0);
    assert_eq!(counts.status_counts.get(&404), Some(&1));
}

#[test]
fn an_empty_batch_counts_nothing() {
    let counts = counts_of(&[]);
    assert_eq!(counts.classified(), 0);
    assert_eq!(counts.success, 0);
    assert_eq!(counts.failed, 0);
}

// =========================================================================
// 失败分类：准备阶段与发送阶段分得开
// =========================================================================

/// 每个用例一份干净的库。
fn mcp_state(tag: &str) -> Arc<AppState> {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-batch-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目、一个模块、一个集合，以及集合下面的一个端点。
///
/// 端点的地址指向 `http://127.0.0.1:1`：没人监听那个端口，故交出去的请求必然连接
/// 失败——用例要断言的正是「发出去了但没拿到响应」这一类。
fn seed(st: &AppState) -> String {
    let project = Project {
        id: "p_batch".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_batch".into(),
        root_id: "p_batch".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let collection = ResourceNode {
        id: "col_batch".into(),
        root_id: "p_batch".into(),
        parent_id: Some("m_batch".into()),
        kind: ResourceNodeType::Collection,
        name: "col".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_batch".into(),
        method: HttpMethod::Get,
        url: "/x".into(),
        base_url_override: Some("http://127.0.0.1:1".into()),
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: Default::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    };
    db::with_tx(&st.pool, |c| {
        db::projects::create(c, &project)?;
        db::permissions::set(c, "p_batch", Permission::Full)?;
        db::nodes::create(c, &module, None)?;
        db::nodes::create(c, &collection, None)?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e_batch".into(),
                root_id: "p_batch".into(),
                parent_id: Some("col_batch".into()),
                kind: ResourceNodeType::Endpoint,
                name: "端点".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&detail),
        )?;
        Ok(())
    })
    .expect("seed");
    "col_batch".into()
}

/// 从工具结果里取出那份 JSON 报告。
fn report_of(result: &CallToolResult) -> Value {
    let text = match &result.content[0] {
        CallToolContent::Text { text } => text.as_str(),
    };
    serde_json::from_str(text).expect("报告是 JSON")
}

/// 树上有端点、库里没有它的请求配置：这条压根没发出去，报告记 `blocked`。
///
/// 它不能记成「没拿到响应」——后者说的是请求交出去了但对方没应答。两句话对调用方
/// 是两种不同的信息：前者是这条没准备好，后者是发出去了没回音。
#[tokio::test]
async fn a_missing_endpoint_detail_is_blocked_not_no_response() {
    let st = mcp_state("missing-detail");
    seed(&st);
    // 造出「树上有端点、库里没有它的请求配置」这个不一致状态。
    db::with_tx(&st.pool, |c| {
        c.execute(
            "DELETE FROM endpoint_details WHERE node_id = ?1",
            ["e_batch"],
        )?;
        Ok(())
    })
    .expect("删掉那一行请求配置");

    let result = tool_api_run_collection(&st, json!({ "collection_id": "col_batch" }))
        .await
        .expect("批量调用本身不失败");
    let report = report_of(&result);

    assert_eq!(report["blocked"], json!(1), "这一条压根没发出去：{report}");
    assert_eq!(
        report["no_response"],
        json!(0),
        "没发出去与没拿到响应是两回事：{report}"
    );
    assert_eq!(report["items"][0]["failure"], json!("blocked"), "{report}");
}

/// 地址指向一个没人监听的端口：请求交出去了但没拿到响应，报告记 `transport`。
#[tokio::test]
async fn a_failed_send_is_counted_as_no_response() {
    let st = mcp_state("failed-send");
    seed(&st);

    let result = tool_api_run_collection(&st, json!({ "collection_id": "col_batch" }))
        .await
        .expect("批量调用本身不失败");
    let report = report_of(&result);

    assert_eq!(
        report["no_response"],
        json!(1),
        "发出去了没拿到响应：{report}"
    );
    assert_eq!(report["blocked"], json!(0), "这一条发出去了：{report}");
    assert_eq!(
        report["items"][0]["failure"],
        json!("transport"),
        "{report}"
    );
}

/// 本次调用的 HTTPS 覆盖对整批生效：形状不对时整批拒绝，而不是给每个端点各记一条
/// 「压根没发出去」。
#[tokio::test]
async fn an_invalid_tls_override_rejects_the_whole_batch() {
    let st = mcp_state("invalid-tls");
    seed(&st);

    let err = tool_api_run_collection(
        &st,
        json!({
            "collection_id": "col_batch",
            "settings_override": { "tls": { "skip_verify": "yes" } },
        }),
    )
    .await
    .expect_err("形状不对的 TLS 覆盖必须让整批失败");

    assert!(
        matches!(err, AppError::Validation(_)),
        "整批拒绝要说清是入参不对：{err:?}"
    );
}
