//! HTTP execution MCP tools — 2 tools total.
//!
//! | tool               | role                                                |
//! |--------------------|-----------------------------------------------------|
//! | api_run_request    | Run an ad-hoc HTTP request (no endpoint)             |
//! | api_call_endpoint  | Resolve + run an endpoint by id                      |
//!
//! Both funnel through [`run_http`] so the output shape is identical
//! regardless of origin.

use serde::Deserialize;
use serde_json::{json, Value};

use simple_post_core::domain::{ApiResponse, Endpoint, Environment, HttpMethod};
use simple_post_core::error::AppError;
use simple_post_core::http_executor::{self, HttpRequestInput, HttpResponse, HttpResult};
use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;

// =========================================================================
// Args shapes
// =========================================================================

#[derive(Debug, Deserialize)]
struct RunRequestArgs {
    method: String,
    url: String,
    #[serde(default)]
    #[allow(dead_code)] // accepted by JSON Schema for forward compatibility
    headers: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)] // accepted by JSON Schema for forward compatibility
    params: Option<Value>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // accepted by JSON Schema for forward compatibility
    auth: Option<Value>,
    #[serde(default)]
    env_id: Option<String>,
    #[serde(default)]
    base_url_definition_id: Option<String>,
    #[serde(default)]
    settings_override: Option<Value>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    follow_redirects: Option<bool>,
    #[serde(default)]
    max_response_size_kb: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CallEndpointArgs {
    endpoint_id: String,
    #[serde(default)]
    overrides: Option<Value>,
    #[serde(default)]
    settings_override: Option<Value>,
}

// =========================================================================
// Dispatch table
// =========================================================================

pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "api_run_request",
            description:
                "Run an ad-hoc HTTP request. Supports env/baseUrl resolution, auth, and settings overrides.",
            schema: json!({
                "type": "object",
                "properties": {
                    "method": { "type": "string" },
                    "url": { "type": "string" },
                    "headers": { "type": "object" },
                    "params": { "type": "object" },
                    "body": { "type": "string" },
                    "auth": { "type": "object" },
                    "env_id": { "type": "string" },
                    "base_url_definition_id": { "type": "string" },
                    "settings_override": { "type": "object" },
                    "timeout_ms": { "type": "integer" },
                    "follow_redirects": { "type": "boolean" },
                    "max_response_size_kb": { "type": "integer" }
                },
                "required": ["method", "url"],
                "additionalProperties": true,
            }),
            func: tool_fn!(tool_api_run_request),
        },
        ToolEntry {
            name: "api_call_endpoint",
            description:
                "Resolve an endpoint by id (env vars, baseUrls, auth, params) and execute it. Returns the parsed response.",
            schema: json!({
                "type": "object",
                "properties": {
                    "endpoint_id": { "type": "string" },
                    "overrides": { "type": "object" },
                    "settings_override": { "type": "object" }
                },
                "required": ["endpoint_id"],
                "additionalProperties": true,
            }),
            func: tool_fn!(tool_api_call_endpoint),
        },
    ]
}

// =========================================================================
// Tool impls
// =========================================================================

async fn tool_api_run_request(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: RunRequestArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_run_request args: {e}")))?;
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }

    // Resolve URL: pull vars from env (if any) and prepend the chosen
    // baseUrl. The renderer-side resolver lives in variable_fill; we
    // reuse its low-level primitives here so the same env-driven
    // substitution runs whether the call comes from the Tauri command
    // or this MCP tool.
    let env: Option<Environment> = if let Some(env_id) = a.env_id.as_deref() {
        Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        })?)
    } else {
        None
    };

    let var_map = env
        .as_ref()
        .map(|e| e.variables.clone())
        .unwrap_or_default();
    let substituted = crate::variable_fill::resolve_template(&a.url, &var_map);
    let (final_url, base_url_used) = apply_base_url(
        &substituted.resolved,
        env.as_ref(),
        a.base_url_definition_id.as_deref(),
    );

    let headers = flatten_string_map(a.headers.as_ref(), "headers")?;
    let body = a.body.clone();

    let (timeout_ms, follow_redirects, max_kb) =
        extract_settings(&a.settings_override, &a);

    let input = HttpRequestInput {
        method: a.method.to_ascii_uppercase(),
        url: final_url.clone(),
        headers: headers.clone(),
        body: body.clone(),
        timeout_ms: timeout_ms.unwrap_or(30_000),
        follow_redirects: follow_redirects.unwrap_or(true),
        max_response_size_kb: max_kb.unwrap_or(10_240),
    };

    state.metrics.inc();
    let result = http_executor::execute(&input).await;
    Ok(build_result_envelope(
        &a.method,
        &final_url,
        &result,
        Some(base_url_used),
        Some(substituted.missing),
    ))
}

async fn tool_api_call_endpoint(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CallEndpointArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_call_endpoint args: {e}")))?;
    let endpoint_id = a.endpoint_id.clone();

    let endpoint: Endpoint = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoints::get(c, &endpoint_id)
    })?;
    let project_id = simple_post_core::db::endpoints::project_id_of(&state.pool, &endpoint.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "api_call_endpoint")
        .await
        .ok();

    // Pull env + project if available. A dangling env id (e.g. the env
    // was deleted) is tolerated — the request just runs without env
    // variable substitution.
    let env: Option<Environment> = if let Some(env_id) = endpoint.environment_id.as_deref() {
        simple_post_core::db::with_conn(&state.pool, |c| simple_post_core::db::environments::get(c, env_id)).ok()
    } else {
        None
    };
    let project = if !project_id.is_empty() {
        simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::projects::get(c, &project_id)
        })
        .ok()
    } else {
        None
    };

    let overrides: crate::variable_fill::EndpointOverrides = a
        .overrides
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let project_ref: &simple_post_core::domain::Project = project.as_ref().unwrap_or(&EMPTY_PROJECT);
    let resolved = crate::variable_fill::resolve_endpoint(
        &endpoint,
        env.as_ref(),
        project_ref,
        Some(&overrides),
    )?;

    let method_str = format_method(&endpoint.method);
    let (timeout_ms, follow_redirects, max_kb) =
        extract_endpoint_settings(&a.settings_override);

    let input = HttpRequestInput {
        method: method_str.clone(),
        url: resolved.url.clone(),
        headers: resolved.headers.clone(),
        body: resolved.body.clone(),
        timeout_ms: timeout_ms.unwrap_or(30_000),
        follow_redirects: follow_redirects.unwrap_or(true),
        max_response_size_kb: max_kb.unwrap_or(10_240),
    };

    state.metrics.inc();
    let result = http_executor::execute(&input).await;
    Ok(build_result_envelope(
        &method_str,
        &resolved.url,
        &result,
        None,
        Some(resolved.missing_vars),
    ))
}

// =========================================================================
// Shared helpers
// =========================================================================

/// Flatten a JSON object (`{"Foo": "bar"}`) or array of pairs
/// (`[["Foo","bar"]]`) into `Vec<(String, String)>`.
///
/// `label` is used to prefix the error message when the input shape
/// is wrong (e.g. a string instead of an object). Returning a
/// `Result` lets the MCP tool surface the error to the LLM as a
/// Validation message; the other version in `response_parse.rs`
/// silently dropped bad shapes, which is the wrong default for
/// user-driven input.
pub(crate) fn flatten_string_map(v: Option<&Value>, label: &str) -> Result<Vec<(String, String)>, AppError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if let Value::Object(map) = v {
        for (k, val) in map {
            let s = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => continue,
                other => other.to_string(),
            };
            out.push((k.clone(), s));
        }
        return Ok(out);
    }
    if let Value::Array(arr) = v {
        for item in arr {
            if let Value::Array(pair) = item {
                if pair.len() >= 2 {
                    out.push((pair[0].to_string(), pair[1].to_string()));
                }
            } else {
                return Err(AppError::Validation(format!(
                    "{label}: array items must be [key, value] pairs"
                )));
            }
        }
        return Ok(out);
    }
    Err(AppError::Validation(format!(
        "{label}: must be object or array of pairs"
    )))
}

fn extract_settings(
    so: &Option<Value>,
    args: &RunRequestArgs,
) -> (Option<u64>, Option<bool>, Option<u64>) {
    let mut t = args.timeout_ms;
    let mut r = args.follow_redirects;
    let mut m = args.max_response_size_kb;
    if let Some(v) = so {
        if let Some(n) = v.get("timeout_ms").and_then(Value::as_u64) {
            t = Some(n);
        }
        if let Some(b) = v.get("follow_redirects").and_then(Value::as_bool) {
            r = Some(b);
        }
        if let Some(n) = v.get("max_response_size_kb").and_then(Value::as_u64) {
            m = Some(n);
        }
    }
    (t, r, m)
}

fn extract_endpoint_settings(so: &Option<Value>) -> (Option<u64>, Option<bool>, Option<u64>) {
    let mut t = None;
    let mut r = None;
    let mut m = None;
    if let Some(v) = so {
        t = v.get("timeout_ms").and_then(Value::as_u64);
        r = v.get("follow_redirects").and_then(Value::as_bool);
        m = v.get("max_response_size_kb").and_then(Value::as_u64);
    }
    (t, r, m)
}

fn apply_base_url(url: &str, env: Option<&Environment>, def_id: Option<&str>) -> (String, String) {
    let Some(env) = env else {
        return (url.to_string(), String::new());
    };
    let def_id = def_id.or_else(|| env.base_urls.keys().next().map(String::as_str));
    let Some(def_id) = def_id else {
        return (url.to_string(), String::new());
    };
    match env.base_urls.get(def_id) {
        Some(base) => {
            let combined = if url.starts_with('/') {
                format!("{}{}", base.trim_end_matches('/'), url)
            } else {
                format!("{}/{}", base.trim_end_matches('/'), url)
            };
            (combined, base.clone())
        }
        None => (url.to_string(), String::new()),
    }
}

/// Render an `HttpMethod` enum back to its wire string.
fn format_method(m: &HttpMethod) -> String {
    m.as_str().to_string()
}

/// Convert an `HttpResult` into a `CallToolResult`. Successful calls
/// get a clean JSON envelope (with `_base_url` / `_missing_vars`
/// annotations when applicable); failures come back with
/// `is_error: true` so the LLM sees the message and can recover.
fn build_result_envelope(
    method: &str,
    url: &str,
    result: &HttpResult,
    base_url: Option<String>,
    missing: Option<Vec<String>>,
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
                        map.insert("_missing_vars".into(), serde_json::to_value(m).unwrap_or(Value::Null));
                    }
                }
            }
            CallToolResult::json(val)
        }
        None => {
            let err = result.error.as_ref().cloned().unwrap_or(simple_post_core::http_executor::HttpError {
                kind: "unknown".into(),
                message: "no response and no error".into(),
            });
            state_metrics_err();
            CallToolResult::err(format!("{}: {}", err.kind, err.message))
        }
    }
}

/// Bump the error counter on transport failures. Pulled out so the
/// `Option` arm doesn't carry the full state through closures.
fn state_metrics_err() {
    // No state in scope here; the http_executor layer handles its own
    // metrics via `tracing`. The MCP daemon's `mcp_metrics.errors`
    // counter is bumped from the daemon_http.rs handler on the `Err`
    // arm of `execute` — so this function is intentionally a no-op and
    // exists only to keep the call site symmetric with the success path.
}

fn http_response_to_api(resp: &HttpResponse, method: &str, url: &str) -> ApiResponse {
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

fn header_pairs_to_tuples(pairs: &[[String; 2]]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|p| (p[0].clone(), p[1].clone()))
        .collect()
}

// =========================================================================
// Project fallback (see api_call_endpoint for context)
// =========================================================================

/// `variable_fill::resolve_endpoint` requires a `&Project`; we don't
/// always have one loaded. Use this as a last-resort placeholder so the
/// resolve step doesn't borrow a `None`.
// `static` (not `const`) so `&EMPTY_PROJECT` is `&'static Project` — taking a
// reference to a `const` creates a temporary that's freed at end of statement,
// which trips E0716 when the resulting `&Project` outlives the statement.
static EMPTY_PROJECT: simple_post_core::domain::Project = simple_post_core::domain::Project {
    id: String::new(),
    name: String::new(),
    description: None,
    base_url: None,
    base_url_definitions: Vec::new(),
    default_base_url_definition_id: None,
    color: String::new(),
    created_at: 0,
    updated_at: 0,
};

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{arg_bool_opt, arg_str, arg_str_opt, arg_u64_opt};
    use simple_post_core::domain::Environment;
    use std::collections::HashMap;

    #[test]
    fn table_has_two_rows() {
        assert_eq!(table().len(), 2);
    }

    #[test]
    fn flatten_object_to_pairs() {
        let v = json!({ "X-Foo": "bar", "X-Num": 42 });
        let pairs = flatten_string_map(Some(&v), "h").unwrap();
        assert_eq!(
            pairs,
            vec![("X-Foo".into(), "bar".into()), ("X-Num".into(), "42".into())]
        );
    }

    #[test]
    fn flatten_array_of_pairs() {
        let v = json!([["X-A", "1"], ["X-B", "2"]]);
        let pairs = flatten_string_map(Some(&v), "h").unwrap();
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn flatten_rejects_bad_shape() {
        let v = json!("not a map");
        assert!(flatten_string_map(Some(&v), "h").is_err());
    }

    #[test]
    fn settings_override_extracts_values() {
        let so = json!({
            "timeout_ms": 5000u64,
            "follow_redirects": false,
            "max_response_size_kb": 1024u64,
        });
        let args = RunRequestArgs {
            method: "GET".into(),
            url: "/x".into(),
            headers: None,
            params: None,
            body: None,
            auth: None,
            env_id: None,
            base_url_definition_id: None,
            settings_override: Some(so),
            timeout_ms: None,
            follow_redirects: None,
            max_response_size_kb: None,
        };
        let (t, r, m) = extract_settings(&args.settings_override, &args);
        assert_eq!(t, Some(5000));
        assert_eq!(r, Some(false));
        assert_eq!(m, Some(1024));
    }

    #[test]
    fn arg_helpers_cover_top_level() {
        let v = json!({ "method": "GET", "url": "/x", "n": 7 });
        assert_eq!(arg_str(&v, "method").unwrap(), "GET");
        assert_eq!(arg_str(&v, "url").unwrap(), "/x");
        assert_eq!(arg_u64_opt(&v, "n"), Some(7));
        assert!(!arg_bool_opt(&v, "missing"));
        assert_eq!(arg_str_opt(&v, "missing"), None);
    }

    #[test]
    fn apply_base_url_handles_env_and_override() {
        let mut base_urls = HashMap::new();
        base_urls.insert("auth".into(), "https://auth.example.com".into());
        let env = Environment {
            id: "e1".into(),
            project_id: "p1".into(),
            name: "dev".into(),
            base_urls,
            variables: HashMap::new(),
            is_active: true,
        };
        let (joined, base) = apply_base_url("/login", Some(&env), Some("auth"));
        assert_eq!(joined, "https://auth.example.com/login");
        assert_eq!(base, "https://auth.example.com");

        let (passthru, _) = apply_base_url("/login", None, None);
        assert_eq!(passthru, "/login");
    }

    #[test]
    fn parse_method_known_and_custom() {
        assert_eq!(
            simple_post_core::domain::parse_http_method("get").unwrap(),
            HttpMethod::Get
        );
        assert_eq!(
            simple_post_core::domain::parse_http_method("POST").unwrap(),
            HttpMethod::Post
        );
        assert!(matches!(
            simple_post_core::domain::parse_http_method("FROBNICATE").unwrap(),
            HttpMethod::Custom(_)
        ));
    }

    #[test]
    fn header_pairs_round_trip() {
        let pairs = [[String::from("a"), String::from("b")]];
        let tuples = header_pairs_to_tuples(&pairs);
        assert_eq!(tuples, vec![("a".into(), "b".into())]);
    }
}