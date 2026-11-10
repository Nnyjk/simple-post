//! `endpoint_history` —— 读某个接口已记录的调用历史。
//!
//! 这个工具替 agent 回答「这个接口实际返回过什么」，所以断言盯住三件事：行按时间倒序、
//! `limit` 与 `body_chars` 真的裁剪了返回、没记录过的接口回空列表而不是报错。

use std::collections::BTreeMap;

use serde_json::json;

use super::history::tool_endpoint_history;
use super::test_support::fixture;
use crate::mcp::AppState;
use simple_post_core::db::response_history;
use simple_post_core::domain::{HttpMethod, StoredRequestSnapshot, StoredResponse};

/// 一条存好的响应。
fn stored_response(status: u16, body: &str) -> StoredResponse {
    StoredResponse {
        status,
        status_text: "OK".to_string(),
        duration_ms: 12,
        size_bytes: body.len() as u64,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: body.to_string(),
        body_base64: None,
        body_json: None,
        content_type: "application/json".to_string(),
        error: None,
        method: Some(HttpMethod::Get),
        url: Some("http://127.0.0.1:9/x".to_string()),
    }
}

/// 那次发出去的请求。
fn stored_request() -> StoredRequestSnapshot {
    StoredRequestSnapshot {
        method: HttpMethod::Get,
        url: "http://127.0.0.1:9/x".to_string(),
        headers: vec![("accept".to_string(), "*/*".to_string())],
        body: None,
        body_base64: None,
    }
}

/// 往某个接口的历史里追加一行。
fn append_row(st: &AppState, endpoint_id: &str, status: u16, body: &str) {
    simple_post_core::db::with_tx(&st.pool, |c| {
        response_history::append(
            c,
            endpoint_id,
            &stored_response(status, body),
            Some(&stored_request()),
        )
    })
    .expect("写入历史行");
}

/// 工具响应里的那份 JSON。
fn payload(res: &crate::mcp::CallToolResult) -> serde_json::Value {
    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    serde_json::from_str(text).expect("工具响应应当是 JSON")
}

/// 工具已登记，且在能力表里有一行——缺行会被两道门当作「未分类」而拒绝。
#[test]
fn the_tool_is_registered_with_a_capability_row() {
    assert!(
        crate::mcp::registered_tool_names()
            .iter()
            .any(|n| n == "endpoint_history"),
        "endpoint_history 没有登记"
    );
    assert!(
        crate::mcp::required_capability("endpoint_history").is_some(),
        "endpoint_history 缺 REQUIRED_CAPABILITY 行"
    );
}

/// 只读档位下这个工具也要看得见：它是 agent 读接口信息的那条路，而默认档位就是只读。
#[tokio::test]
async fn the_tool_is_visible_under_the_read_only_policy() {
    let (st, _endpoint_id) = fixture();
    let names: Vec<String> = crate::mcp::tool_descriptors(&st)
        .await
        .iter()
        .map(|d| d.name.clone())
        .collect();
    assert!(
        names.iter().any(|n| n == "endpoint_history"),
        "只读档位的工具清单里没有 endpoint_history: {names:?}"
    );
}

/// 读回来的就是那几次调用：状态码、响应体与那次发出去的请求，最新的在前。
#[tokio::test]
async fn endpoint_history_returns_the_recorded_calls_newest_first() {
    let (st, endpoint_id) = fixture();
    append_row(&st, &endpoint_id, 200, r#"{"page":1}"#);
    append_row(&st, &endpoint_id, 500, r#"{"error":"boom"}"#);

    let res = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id }))
        .await
        .expect("读历史");
    let rows = payload(&res);
    let rows = rows.as_array().expect("结果是数组");

    assert_eq!(rows.len(), 2, "两行历史都要回来");
    assert_eq!(rows[0]["status"], 500, "最新的那一行在前");
    assert_eq!(rows[0]["body"], r#"{"error":"boom"}"#);
    assert_eq!(rows[1]["status"], 200);
    assert_eq!(rows[1]["body"], r#"{"page":1}"#);
    assert_eq!(rows[1]["method"], "GET");
    assert_eq!(rows[1]["url"], "http://127.0.0.1:9/x");
    // 那次发出去的请求也回来了：前端接入要知道请求长什么样。
    assert_eq!(rows[1]["request"]["method"], "GET");
    assert_eq!(rows[1]["request"]["url"], "http://127.0.0.1:9/x");
}

/// `limit` 真的裁剪返回的行数。
#[tokio::test]
async fn endpoint_history_honours_the_limit() {
    let (st, endpoint_id) = fixture();
    append_row(&st, &endpoint_id, 200, "a");
    append_row(&st, &endpoint_id, 201, "b");
    append_row(&st, &endpoint_id, 202, "c");

    let res = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id, "limit": 2 }))
        .await
        .expect("读历史");
    let rows = payload(&res);
    assert_eq!(rows.as_array().expect("结果是数组").len(), 2);
}

/// `body_chars` 真的把 body 裁短，并且说清楚那是片段。
#[tokio::test]
async fn endpoint_history_clips_the_body_and_says_so() {
    let (st, endpoint_id) = fixture();
    append_row(&st, &endpoint_id, 200, "0123456789");

    let res = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id, "body_chars": 4 }))
        .await
        .expect("读历史");
    let rows = payload(&res);
    let rows = rows.as_array().expect("结果是数组");
    assert_eq!(rows[0]["body"], "0123");
    assert_eq!(rows[0]["bodyTruncated"], true, "片段必须说清楚是片段");
}

/// `body_chars` 给 0 时不给 body，但仍说清楚那是片段。
#[tokio::test]
async fn endpoint_history_can_return_the_rows_without_any_body() {
    let (st, endpoint_id) = fixture();
    append_row(&st, &endpoint_id, 200, "0123456789");

    let res = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id, "body_chars": 0 }))
        .await
        .expect("读历史");
    let rows = payload(&res);
    let rows = rows.as_array().expect("结果是数组");
    assert_eq!(rows[0]["body"], "");
    assert_eq!(rows[0]["bodyTruncated"], true);
}

/// 没发过的接口是空历史，不是缺失资源。
#[tokio::test]
async fn endpoint_history_of_an_endpoint_without_calls_is_an_empty_list() {
    let (st, endpoint_id) = fixture();

    let res = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id }))
        .await
        .expect("读历史");
    let rows = payload(&res);
    assert_eq!(rows.as_array().expect("结果是数组").len(), 0);
}

/// 找不到的接口报错：它不是一个「空历史」的接口。
#[tokio::test]
async fn endpoint_history_rejects_an_unknown_endpoint() {
    let (st, _endpoint_id) = fixture();

    let err = tool_endpoint_history(&st, json!({ "endpoint_id": "e_nope" }))
        .await
        .expect_err("不存在的接口必须报错");
    assert!(
        matches!(err, simple_post_core::error::AppError::NotFound(_)),
        "got: {err:?}"
    );
}

/// 参数名拼错时报错，而不是当作「没传这个限制」。
#[tokio::test]
async fn endpoint_history_rejects_an_unknown_argument() {
    let (st, endpoint_id) = fixture();

    let err = tool_endpoint_history(&st, json!({ "endpoint_id": endpoint_id, "limitt": 3 }))
        .await
        .expect_err("拼错的字段名必须报错");
    assert!(
        matches!(err, simple_post_core::error::AppError::Validation(_)),
        "got: {err:?}"
    );
}
