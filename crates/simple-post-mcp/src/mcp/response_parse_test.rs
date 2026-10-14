use super::*;
use crate::mcp::arg_str_opt;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::Instant;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-resp-parse-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(crate::mcp::AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 取出 `CallToolResult` 里的 JSON 载荷。
///
/// 工具结果把 JSON 值包成一个 `Text` content 条目，所以 `serde_json::to_string(&res)`
/// 会二次序列化：body 里的每个引号都变成转义形式。与其做子串匹配，不如把内层字符串取
/// 出来再解析成 `Value`。
fn extract_text_json(res: &CallToolResult) -> serde_json::Value {
    let wrapped: serde_json::Value = serde_json::to_value(res).unwrap();
    // CallToolResult 形状：{ content: [ { type: "text", text: "<json>" } ], is_error: bool }
    let text = wrapped["content"][0]["text"]
        .as_str()
        .expect("text content present");
    serde_json::from_str(text).expect("inner content is JSON")
}

#[test]
fn table_has_three_rows() {
    assert_eq!(table().len(), 3);
}

#[tokio::test]
async fn parse_response_handles_json_body() {
    let res = tool_api_parse_response(
        &state(),
        json!({
            "body": r#"{"name":"alice","age":30}"#,
            "content_type": "application/json",
            "status": 200
        }),
    )
    .await
    .unwrap();
    let s = serde_json::to_string(&res).unwrap();
    assert!(s.contains("alice"));
    assert!(s.contains("30"));
}

#[tokio::test]
async fn parse_response_handles_text_body() {
    let res = tool_api_parse_response(
        &state(),
        json!({
            "body": "plain text",
            "content_type": "text/plain",
            "status": 200
        }),
    )
    .await
    .unwrap();
    let s = serde_json::to_string(&res).unwrap();
    assert!(s.contains("plain text"));
}

#[tokio::test]
async fn diff_responses_flags_status_change() {
    let res = tool_api_diff_responses(
        &state(),
        json!({
            "a": { "status": 200, "content_type": "application/json", "body": "{}" },
            "b": { "status": 500, "content_type": "application/json", "body": "{}" }
        }),
    )
    .await
    .unwrap();
    let inner = extract_text_json(&res);
    assert_eq!(inner["status_changed"], json!(true));
}

#[tokio::test]
async fn diff_responses_flags_body_change() {
    let res = tool_api_diff_responses(
        &state(),
        json!({
            "a": { "status": 200, "content_type": "application/json", "body": "{\"x\":1}" },
            "b": { "status": 200, "content_type": "application/json", "body": "{\"x\":2}" }
        }),
    )
    .await
    .unwrap();
    let inner = extract_text_json(&res);
    assert_eq!(inner["body_changed"], json!(true));
}

#[tokio::test]
async fn extract_jsonpath_finds_value() {
    let res = tool_api_extract_jsonpath(
        &state(),
        json!({
            "body": r#"{"users":[{"name":"alice"},{"name":"bob"}]}"#,
            "content_type": "application/json",
            "path": "$.users[0].name"
        }),
    )
    .await
    .unwrap();
    let inner = extract_text_json(&res);
    assert_eq!(inner["found"], json!(true));
    assert_eq!(inner["value"], json!("alice"));
}

#[tokio::test]
async fn extract_jsonpath_returns_not_found_when_missing() {
    let res = tool_api_extract_jsonpath(
        &state(),
        json!({
            "body": r#"{"a":1}"#,
            "content_type": "application/json",
            "path": "$.does.not.exist"
        }),
    )
    .await
    .unwrap();
    let inner = extract_text_json(&res);
    assert_eq!(inner["found"], json!(false));
}

#[tokio::test]
async fn extract_jsonpath_rejects_non_json_content_type() {
    let res = tool_api_extract_jsonpath(
        &state(),
        json!({
            "body": "not json",
            "content_type": "text/plain",
            "path": "$.x"
        }),
    )
    .await
    .unwrap();
    // 错误路径返回 is_error=true
    assert!(res.is_error);
}

#[tokio::test]
async fn extract_jsonpath_rejects_empty_path() {
    let res = tool_api_extract_jsonpath(
        &state(),
        json!({
            "body": "{}",
            "content_type": "application/json",
            "path": ""
        }),
    )
    .await;
    assert!(matches!(res, Err(AppError::Validation(_))));
}

#[test]
fn headers_from_value_handles_object_and_array() {
    let obj = json!({ "X-A": "1", "X-B": "2" });
    let pairs = crate::mcp::http_ops::flatten_string_map(Some(&obj), "h").unwrap();
    assert_eq!(pairs.len(), 2);

    let arr = json!([["X-A", "1"]]);
    let pairs = crate::mcp::http_ops::flatten_string_map(Some(&arr), "h").unwrap();
    assert_eq!(pairs.len(), 1);

    assert!(crate::mcp::http_ops::flatten_string_map(None, "h")
        .unwrap()
        .is_empty());
    assert!(
        crate::mcp::http_ops::flatten_string_map(Some(&Value::Null), "h")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn arg_str_opt_returns_none_for_missing() {
    let v = json!({});
    assert_eq!(arg_str_opt(&v, "x"), None);
}
