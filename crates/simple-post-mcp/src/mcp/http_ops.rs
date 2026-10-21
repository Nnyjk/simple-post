//! HTTP 执行类 MCP 工具 —— 共 2 个：`api_run_request`（临时请求，不挂端点）与
//! `api_call_endpoint`（按 id 解析并发出端点）。
//!
//! 两者汇到同一条执行与结果封装路径，所以不论来源，输出形状一致。本文件只放登记、
//! 入参形状与两个 handler；入参的压平在 `input.rs`，结果信封在 `envelope.rs`。
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::{EndpointDetail, Environment};
use simple_post_core::error::AppError;
use simple_post_core::http_executor::{self, HttpRequestInput};

use self::envelope::build_result_envelope;
use self::input::{apply_base_url, extract_endpoint_settings, extract_settings, format_method};

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
        None,
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

    // multipart 在这里才成形：boundary 必须现生成，Content-Type 与 body 用同一个值。
    // `resolve_endpoint` 只做变量替换，故它给不出带 boundary 的 Content-Type。
    //
    // 判据读 `resolved.body_mode`（**套过 overrides 的那个 mode**），不读端点行里存的那个：
    // 调用方给 `overrides.body` 就能换掉 mode，而 Content-Type 那边读的是
    // `resolve_endpoint` 内部同一个 eff_body.mode。两边读不同的值会出两种坏结果——
    // 存 json / 覆盖 multipart 时整条请求没有 Content-Type 且分片原样发出；存 multipart /
    // 覆盖 json 时 body 被换成只有收尾分隔符的壳，调用方传的内容静默消失。
    let mut body = resolved.body.clone();
    if resolved.body_mode == simple_post_core::domain::BodyMode::Multipart {
        body = body
            .take()
            .map(|content| multipart::apply(&mut headers, &content));
    }

    // 脚本链跑在同一条序列里：前置脚本改写请求 → 发出 → 测试脚本断言并写回变量。
    // 整条序列由 core 一处实现，故桌面端与 MCP 端不会分叉。
    //
    // 变量池与 `resolve_endpoint` 同一套规则：**停用的变量不参与**，脚本看到的
    // 环境与这次请求看到的是同一个。
    let variables: std::collections::BTreeMap<String, String> = env
        .as_ref()
        .map(|e| {
            e.variables
                .iter()
                .filter(|(k, _)| !e.disabled_variables.iter().any(|d| d == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();

    let outcome = simple_post_core::script::send_with_scripts(
        &state.pool,
        simple_post_core::script::ScriptedSendInput {
            endpoint_id: Some(endpoint.node_id.clone()),
            request: simple_post_core::script::ScriptRequest {
                method: method_str.clone(),
                url: url.clone(),
                headers: headers.clone(),
                body,
            },
            env: simple_post_core::script::EnvSnapshot {
                id: env.as_ref().map(|e| e.id.clone()),
                variables,
            },
            settings: simple_post_core::domain::RequestSettings {
                timeout_ms: timeout_ms.unwrap_or(30_000),
                follow_redirects: follow_redirects.unwrap_or(true),
                max_response_size_kb: max_kb.unwrap_or(10_240),
            },
            // MCP 端没有「界面上未保存的草稿」这回事，用的就是存的那份脚本。
            script_override: None,
        },
    )
    .await?;

    state.metrics.inc();
    Ok(build_result_envelope(
        &method_str,
        &outcome.request.url,
        &outcome.http,
        None,
        Some(resolved.missing_vars),
        outcome.script,
    ))
}

// =========================================================================
// Project 占位回退
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
// 子模块
// =========================================================================

mod envelope;
mod input;
mod multipart;

pub(crate) use input::flatten_string_map;

// =========================================================================
// 测试
// =========================================================================
//
// 兄弟 `#[path]` 模块，遵循本 crate 的惯例：Rust 按模块路径找文件，所以测试专用
// *目录* 是找不到的。`http_ops_test.rs` 是纯单元断言；oauth2 那组要起两个真实的
// loopback socket，放在 `http_ops/oauth2_test.rs`。

#[cfg(test)]
#[path = "http_ops_test.rs"]
mod tests;

#[cfg(test)]
#[path = "http_ops/oauth2_test.rs"]
mod oauth2_tests;

#[cfg(test)]
#[path = "http_ops/multipart_mode_test.rs"]
mod multipart_mode_tests;

#[cfg(test)]
#[path = "http_ops/script_test.rs"]
mod script_tests;
