//! Response-parsing MCP tools — 3 tools total.
//!
//! | tool                  | role                                                  |
//! |-----------------------|-------------------------------------------------------|
//! | api_parse_response    | Parse a raw HTTP response body into a structured form |
//! | api_diff_responses    | Diff two parsed responses                            |
//! | api_extract_jsonpath  | Run a JSONPath query against a response body          |
//!
//! MCP wrapper over `crate::response_parse`. Adds JSON-RPC I/O around the
//! base layer's pure transforms.

use serde::Deserialize;
use serde_json::{json, Value};

use simple_post_core::domain::ApiResponse;
use simple_post_core::error::AppError;
use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;

// =========================================================================
// Args shapes
// =========================================================================

#[derive(Debug, Deserialize)]
struct ParseResponseArgs {
    body: String,
    content_type: String,
    status: u16,
    #[serde(default)]
    headers: Option<Value>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiffResponsesArgs {
    a: ResponseInput,
    b: ResponseInput,
}

#[derive(Debug, Deserialize)]
struct ResponseInput {
    status: u16,
    content_type: String,
    #[serde(default)]
    headers: Option<Value>,
    body: String,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExtractJsonPathArgs {
    body: String,
    content_type: String,
    path: String,
}

// =========================================================================
// Dispatch table
// =========================================================================

pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "api_parse_response",
            description:
                "Parse a raw HTTP response into a structured ParsedResponse (parsed body, headers map, content-type detection).",
            schema: json!({
                "type": "object",
                "properties": {
                    "body": { "type": "string" },
                    "content_type": { "type": "string" },
                    "status": { "type": "integer" },
                    "headers": { "type": "object" },
                    "duration_ms": { "type": "integer" },
                    "method": { "type": "string" },
                    "url": { "type": "string" }
                },
                "required": ["body", "content_type", "status"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_api_parse_response),
        },
        ToolEntry {
            name: "api_diff_responses",
            description:
                "Compare two parsed responses and report status, body, headers, and JSONPath differences.",
            schema: json!({
                "type": "object",
                "properties": {
                    "a": {
                        "type": "object",
                        "properties": {
                            "status": { "type": "integer" },
                            "content_type": { "type": "string" },
                            "headers": { "type": "object" },
                            "body": { "type": "string" },
                            "duration_ms": { "type": "integer" }
                        },
                        "required": ["status", "content_type", "body"]
                    },
                    "b": {
                        "type": "object",
                        "properties": {
                            "status": { "type": "integer" },
                            "content_type": { "type": "string" },
                            "headers": { "type": "object" },
                            "body": { "type": "string" },
                            "duration_ms": { "type": "integer" }
                        },
                        "required": ["status", "content_type", "body"]
                    }
                },
                "required": ["a", "b"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_api_diff_responses),
        },
        ToolEntry {
            name: "api_extract_jsonpath",
            description:
                "Run a JSONPath expression against a response body. Returns the matched value(s).",
            schema: json!({
                "type": "object",
                "properties": {
                    "body": { "type": "string" },
                    "content_type": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["body", "content_type", "path"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_api_extract_jsonpath),
        },
    ]
}

// =========================================================================
// Tool impls
// =========================================================================

async fn tool_api_parse_response(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ParseResponseArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_parse_response args: {e}")))?;
    let _ = state; // No project gate — pure transform.

    let headers = crate::mcp::http_ops::flatten_string_map(a.headers.as_ref(), "headers")?;
    let parsed =
        crate::response_parse::parse(&a.body, &a.content_type, a.status, &headers);

    let api = ApiResponse {
        status: a.status,
        status_text: parsed.status_text.clone(),
        duration_ms: a.duration_ms.unwrap_or(0),
        size_bytes: parsed.size_bytes,
        headers: headers
            .iter()
            .map(|(k, v)| [k.clone(), v.clone()])
            .collect(),
        body: a.body,
        body_json: parsed.body_json,
        content_type: a.content_type,
        error: None,
        method: a
            .method
            .as_deref()
            .and_then(|m| simple_post_core::domain::parse_http_method(m).ok()),
        url: a.url,
    };

    Ok(CallToolResult::json(api))
}

async fn tool_api_diff_responses(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: DiffResponsesArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_diff_responses args: {e}")))?;
    let _ = state;

    let api_a = build_api(&a.a)?;
    let api_b = build_api(&a.b)?;
    let diff = crate::response_parse::diff(&api_a, &api_b);

    let mut val = json!({
        "status_changed": api_a.status != api_b.status,
        "status_a": api_a.status,
        "status_b": api_b.status,
        "body_changed": api_a.body != api_b.body,
        "body_size_a": api_a.size_bytes,
        "body_size_b": api_b.size_bytes,
        "headers_changed": header_diff(&api_a, &api_b),
        "added_paths": diff.added_paths,
        "removed_paths": diff.removed_paths,
        "changed_paths": diff.changed_paths,
    });
    // Surface the body_json on both sides as a convenience so the LLM
    // can pattern-match without doing its own reparse.
    if let Value::Object(map) = &mut val {
        map.insert(
            "body_json_a".into(),
            api_a.body_json.clone().unwrap_or(Value::Null),
        );
        map.insert(
            "body_json_b".into(),
            api_b.body_json.clone().unwrap_or(Value::Null),
        );
    }
    Ok(CallToolResult::json(val))
}

async fn tool_api_extract_jsonpath(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ExtractJsonPathArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_extract_jsonpath args: {e}")))?;
    let _ = state;

    if a.path.trim().is_empty() {
        return Err(AppError::Validation("path must not be empty".into()));
    }

    match crate::response_parse::extract_jsonpath(&a.body, &a.content_type, &a.path) {
        Some(v) => Ok(CallToolResult::json(json!({
            "found": true,
            "value": v,
        }))),
        None => {
            // Lenient: "no match" is not an error. Surface as found=false
            // so the LLM can branch without parsing an error message.
            if !a.content_type.to_ascii_lowercase().contains("application/json") {
                Ok(CallToolResult::err(
                    "extract_jsonpath requires application/json content_type",
                ))
            } else {
                Ok(CallToolResult::json(json!({
                    "found": false,
                    "value": Value::Null,
                })))
            }
        }
    }
}

// =========================================================================
// Helpers
// =========================================================================

fn build_api(input: &ResponseInput) -> Result<ApiResponse, AppError> {
    let headers = crate::mcp::http_ops::flatten_string_map(input.headers.as_ref(), "headers")?;
    let body_json = if input
        .content_type
        .to_ascii_lowercase()
        .contains("application/json")
    {
        serde_json::from_str(&input.body).ok()
    } else {
        None
    };
    let method = input
        .method
        .as_deref()
        .and_then(|m| simple_post_core::domain::parse_http_method(m).ok());
    Ok(ApiResponse {
        status: input.status,
        status_text: crate::response_parse::status_reason(input.status).to_string(),
        duration_ms: input.duration_ms.unwrap_or(0),
        size_bytes: input.body.len() as u64,
        headers: headers
            .iter()
            .map(|(k, v)| [k.clone(), v.clone()])
            .collect(),
        body: input.body.clone(),
        body_json,
        content_type: input.content_type.clone(),
        error: None,
        method,
        url: input.url.clone(),
    })
}

fn header_diff(a: &ApiResponse, b: &ApiResponse) -> Vec<String> {
    let mut changed = Vec::new();
    for ak in a.headers.iter().map(|p| p[0].clone()) {
        let a_v = a
            .headers
            .iter()
            .find(|p| p[0] == ak)
            .map(|p| p[1].clone())
            .unwrap_or_default();
        let b_v = b
            .headers
            .iter()
            .find(|p| p[0] == ak)
            .map(|p| p[1].clone())
            .unwrap_or_default();
        if a_v != b_v {
            changed.push(ak);
        }
    }
    changed
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::arg_str_opt;
    use std::sync::{atomic::{AtomicU32, Ordering}, Arc};
    use std::time::Instant;

    /// Per-call counter that disambiguates temp dirs when several tests
    /// construct their state() in the same millisecond (cargo's default
    /// runs tests in parallel).
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

    /// Decode the inner JSON payload inside a `CallToolResult`.
    ///
    /// Tool results wrap their JSON value as a single `Text` content
    /// entry, so `serde_json::to_string(&res)` doubles-serializes: every
    /// `"` inside the body becomes `\"`. Rather than rely on substring
    /// matching (which the previous test relied on and was wrong),
    /// pull the inner string out, then parse it into `Value`.
    fn extract_text_json(res: &CallToolResult) -> serde_json::Value {
        let wrapped: serde_json::Value = serde_json::to_value(res).unwrap();
        // CallToolResult shape: { content: [ { type: "text", text: "<json>" } ], is_error: bool }
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
        // Error path returns is_error=true
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
        let pairs =
            crate::mcp::http_ops::flatten_string_map(Some(&obj), "h").unwrap();
        assert_eq!(pairs.len(), 2);

        let arr = json!([["X-A", "1"]]);
        let pairs =
            crate::mcp::http_ops::flatten_string_map(Some(&arr), "h").unwrap();
        assert_eq!(pairs.len(), 1);

        assert!(crate::mcp::http_ops::flatten_string_map(None, "h")
            .unwrap()
            .is_empty());
        assert!(crate::mcp::http_ops::flatten_string_map(Some(&Value::Null), "h")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn arg_str_opt_returns_none_for_missing() {
        let v = json!({});
        assert_eq!(arg_str_opt(&v, "x"), None);
    }
}