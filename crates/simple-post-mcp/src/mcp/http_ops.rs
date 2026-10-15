//! HTTP 执行类 MCP 工具 —— 共 2 个：`api_run_request`（临时请求，不挂端点）与
//! `api_call_endpoint`（按 id 解析并发出端点）。
//!
//! 两者汇到同一条执行与结果封装路径，所以不论来源，输出形状一致。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::{ApiResponse, EndpointDetail, Environment, HttpMethod};
use simple_post_core::error::AppError;
use simple_post_core::http_executor::{self, HttpRequestInput, HttpResponse, HttpResult};

// =========================================================================
// Args 形状
// =========================================================================

#[derive(Debug, Deserialize)]
struct RunRequestArgs {
    method: String,
    url: String,
    #[serde(default)]
    #[allow(dead_code)] // JSON Schema 为向前兼容而接受，当前未实现
    headers: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)] // JSON Schema 为向前兼容而接受，当前未实现
    params: Option<Value>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // JSON Schema 为向前兼容而接受，当前未实现
    auth: Option<Value>,
    #[serde(default)]
    env_id: Option<String>,
    #[serde(default)]
    base_url_definition_id: Option<String>,
    /// `base_url_definition_id` 取值时读的是哪个模块的格子。URL 住在
    /// (环境 × 服务 × 模块)，而本工具没有端点可借模块，不给就没有格子、没有
    /// base URL。
    #[serde(default)]
    module_id: Option<String>,
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
// dispatch 表
// =========================================================================

pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "api_run_request",
            description:
                "Run an ad-hoc HTTP request. Supports env/baseUrl resolution, auth, and settings overrides. A base URL is read from moduleBaseUrls[base_url_definition_id][module_id], so both ids are needed to prepend one.",
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
                    "module_id": { "type": "string" },
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
// 工具实现
// =========================================================================

async fn tool_api_run_request(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: RunRequestArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_run_request args: {e}")))?;
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }

    // 解析 URL：取 env 变量并前置选定的 baseUrl。解析器本体在 variable_fill，
    // 这里复用它的底层原语，好让同一次 env 驱动的替换在 Tauri 命令与本工具
    // 两条路上跑出一致结果。
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
        a.module_id.as_deref(),
    );

    let headers = flatten_string_map(a.headers.as_ref(), "headers")?;
    let body = a.body.clone();

    let (timeout_ms, follow_redirects, max_kb) = extract_settings(&a.settings_override, &a);

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

async fn tool_api_call_endpoint(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: CallEndpointArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_call_endpoint args: {e}")))?;
    let endpoint_id = a.endpoint_id.clone();

    let endpoint: EndpointDetail = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, &endpoint_id)
    })?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &endpoint.node_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "api_call_endpoint").await?;

    // env / project 取不到就降级：悬空的 env id（例如环境已被删）不至于让调用
    // 失败，只是这次请求不做变量替换。
    let env: Option<Environment> = if let Some(env_id) = endpoint.environment_id.as_deref() {
        simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        })
        .ok()
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

    // 端点属于哪个模块只在树里，detail 行上没有，得查 `nodes` 补齐；但调用方
    // 自己指定了就不覆盖 —— `resolve_endpoint` 是纯函数，别让它被一次查库改写。
    // 不补的结果是服务列表收窄到空，请求裸奔没有 base URL。
    let mut overrides = overrides;
    if overrides.module_id.is_none() {
        overrides.module_id = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::module_id_of(c, &endpoint.node_id)
        })
        .ok()
        .flatten();
    }

    let project_ref: &simple_post_core::domain::Project =
        project.as_ref().unwrap_or(&EMPTY_PROJECT);
    let resolved = crate::variable_fill::resolve_endpoint(
        &endpoint,
        env.as_ref(),
        project_ref,
        Some(&overrides),
    )?;

    let method_str = format_method(&endpoint.method);
    let (timeout_ms, follow_redirects, max_kb) = extract_endpoint_settings(&a.settings_override);

    // oauth2 的 token 是发请求之前才取到的，`resolve_endpoint`（纯函数）拿不到它，
    // 故注入放在这里而不是那里——`build_headers` 对 bearer/basic/apikey 的处理不受影响。
    //
    // **必须同时匹配 `kind`**：`resolve_auth` 对未选中的方案原样保留载荷，所以一个从
    // oauth2 切到 bearer 的端点仍带着 `oauth2: Some(..)`。只匹配载荷时 header 档碰巧
    // 被 `build_headers` 已写好的 `Authorization` 挡住，而 query 档**不挡**——一个用户
    // 已经不再选择的 token 会被拼进 URL 发出去。
    let mut url = resolved.url.clone();
    let mut headers = resolved.headers.clone();
    if let simple_post_core::domain::AuthConfig {
        kind: simple_post_core::domain::AuthType::Oauth2,
        oauth2: Some(cfg),
        ..
    } = &resolved.auth
    {
        let token = simple_post_core::oauth2::ensure_token(&state.pool, cfg).await?;
        simple_post_core::oauth2::inject(cfg, &token, &mut headers, &mut url);
    }

    let input = HttpRequestInput {
        method: method_str.clone(),
        url: url.clone(),
        headers: headers.clone(),
        body: resolved.body.clone(),
        timeout_ms: timeout_ms.unwrap_or(30_000),
        follow_redirects: follow_redirects.unwrap_or(true),
        max_response_size_kb: max_kb.unwrap_or(10_240),
    };

    state.metrics.inc();
    let result = http_executor::execute(&input).await;
    Ok(build_result_envelope(
        &method_str,
        &url,
        &result,
        None,
        Some(resolved.missing_vars),
    ))
}

// =========================================================================
// 共享辅助函数
// =========================================================================

/// 把 JSON 对象（`{"Foo": "bar"}`）或键值对数组（`[["Foo","bar"]]`）压平成
/// `Vec<(String, String)>`。
///
/// `label` 用来给形状错误的提示加前缀。这里返回 `Result` 而不是静默丢弃：
/// 输入来自 AI 客户端，静默丢形状等于凭空造一个「成功但少了几个头」的结果。
pub(crate) fn flatten_string_map(
    v: Option<&Value>,
    label: &str,
) -> Result<Vec<(String, String)>, AppError> {
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

/// 从活的取值层读一个 base URL 前置上去。
///
/// `module_base_urls[def_id][module_id]` 是 URL 唯一住址（见
/// `variable_fill::resolve_base_url_with`），两个键都必须给，且**刻意没有兜底**：
/// 回退到 `env.base_urls` 那列死列、或「取第一个能用的键」，都等于把一个任意的
/// 服务怼到这次请求上。env、服务、模块、格子缺任意一个都表示「没有 base URL」，
/// 按调用方给的路径原样发出。
fn apply_base_url(
    url: &str,
    env: Option<&Environment>,
    def_id: Option<&str>,
    module_id: Option<&str>,
) -> (String, String) {
    let (Some(env), Some(def_id), Some(module_id)) = (env, def_id, module_id) else {
        return (url.to_string(), String::new());
    };
    match env
        .module_base_urls
        .get(def_id)
        .and_then(|cells| cells.get(module_id))
    {
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

/// `HttpMethod` 还原成线上字符串。
fn format_method(m: &HttpMethod) -> String {
    m.as_str().to_string()
}

/// `HttpResult` → `CallToolResult`。成功给干净的 JSON 信封（必要时附
/// `_base_url` / `_missing_vars`）；失败走 `is_error: true`，LLM 读得到消息才能恢复。
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
                        map.insert(
                            "_missing_vars".into(),
                            serde_json::to_value(m).unwrap_or(Value::Null),
                        );
                    }
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
    pairs.iter().map(|p| (p[0].clone(), p[1].clone())).collect()
}

// =========================================================================
// Project 占位兜底
// =========================================================================

// `resolve_endpoint` 要一个 `&Project`，而项目不一定查得到；查不到就用这个空壳，
// 免得解析这一步去 borrow 一个 `None`。
// 必须是 `static` 而非 `const`：`&EMPTY_PROJECT` 要拿到的是 `&'static Project`，
// 引用 `const` 会产生语句结束即释放的临时值，存活的 `&Project` 会触发 E0716。
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
// 测试
// =========================================================================

#[cfg(test)]
#[path = "http_ops_test.rs"]
mod tests;
