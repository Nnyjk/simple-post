//! `HttpResult` → `CallToolResult` 的结果封装。
//!
//! 成功给干净的 JSON 信封（必要时附 `_base_url` / `_missing_vars`）；失败走
//! `is_error: true`，LLM 读得到消息才能恢复。两个工具共用，所以输出形状一致。

use serde_json::Value;

use crate::mcp::CallToolResult;
use simple_post_core::domain::ApiResponse;
use simple_post_core::domain::ScriptOutcome;
use simple_post_core::http_executor::{HttpResponse, HttpResult};
/// `HttpResult` → `CallToolResult`。成功给干净的 JSON 信封（必要时附
/// `_base_url` / `_missing_vars`）；失败走 `is_error: true`，LLM 读得到消息才能恢复。
pub(super) fn build_result_envelope(
    method: &str,
    url: &str,
    result: &HttpResult,
    base_url: Option<String>,
    missing: Option<Vec<String>>,
    script: Option<ScriptOutcome>,
) -> CallToolResult {
    match &result.response {
        Some(resp) => {
            let api = http_response_to_api(resp, method, url);
            let parsed = crate::response_parse::parse(
                &api.body,
                &api.content_type,
                api.status,
                &header_pairs_to_tuples(&api.headers),
            );
            let mut val = serde_json::json!({
                "ok": true,
                "status": api.status,
                "status_text": parsed.status_text,
                "duration_ms": api.duration_ms,
                "size_bytes": parsed.size_bytes,
                "headers": api.headers,
                "body": api.body,
                "body_json": parsed.body_json,
                "content_type": api.content_type,
                "method": api.method,
                "url": api.url,
                "truncated": resp.truncated,
            });
            if let Some(b) = base_url {
                if !b.is_empty() {
                    if let Value::Object(map) = &mut val {
                        map.insert("_base_url".into(), Value::String(b));
                    }
                }
            }
            if let Some(m) = missing {
                if !m.is_empty() {
                    if let Value::Object(map) = &mut val {
                        map.insert(
                            "_missing_vars".into(),
                            serde_json::to_value(m).unwrap_or(Value::Null),
                        );
                    }
                }
            }
            // 脚本的产出：断言、日志、错误与写回的环境变量。
            // 脚本失败**不**改写 `ok` —— 请求本身成了就是成了，脚本的事单独说。
            if let Some(s) = script {
                if let Value::Object(map) = &mut val {
                    map.insert(
                        "_script".into(),
                        serde_json::to_value(s).unwrap_or(Value::Null),
                    );
                }
            }
            CallToolResult::json(val)
        }
        None => {
            let err = result.error.as_ref().cloned().unwrap_or(
                simple_post_core::http_executor::HttpError {
                    kind: "unknown".into(),
                    message: "no response and no error".into(),
                },
            );
            state_metrics_err();
            CallToolResult::err(format!("{}: {}", err.kind, err.message))
        }
    }
}

/// 传输失败时的错误计数。单独抽出来是为了让 `Option` 分支不必把整个 state
/// 带进闭包。
fn state_metrics_err() {
    // 这里拿不到 state：`http_executor` 自己用 `tracing` 记指标，daemon 的
    // `mcp_metrics.errors` 由 daemon_http.rs 的处理器自增。本函数刻意为空，只为让
    // 成功/失败两侧的调用点对称。
}

pub(super) fn http_response_to_api(resp: &HttpResponse, method: &str, url: &str) -> ApiResponse {
    ApiResponse {
        status: resp.status,
        status_text: resp.status_text.clone(),
        duration_ms: resp.duration_ms as u64,
        size_bytes: resp.size_bytes,
        headers: resp.headers.clone(),
        body: resp.body.clone(),
        body_json: None,
        content_type: resp.content_type.clone(),
        error: None,
        method: simple_post_core::domain::parse_http_method(method).ok(),
        url: Some(url.to_string()),
    }
}

pub(super) fn header_pairs_to_tuples(pairs: &[[String; 2]]) -> Vec<(String, String)> {
    pairs.iter().map(|p| (p[0].clone(), p[1].clone())).collect()
}
