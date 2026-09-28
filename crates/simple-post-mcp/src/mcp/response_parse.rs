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

use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::ApiResponse;
use simple_post_core::error::AppError;

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
    let parsed = crate::response_parse::parse(&a.body, &a.content_type, a.status, &headers);

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
            if !a
                .content_type
                .to_ascii_lowercase()
                .contains("application/json")
            {
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
#[path = "response_parse_test.rs"]
mod tests;
