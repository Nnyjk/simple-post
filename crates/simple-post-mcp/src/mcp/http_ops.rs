//! HTTP 执行类 MCP 工具 —— 共 2 个：`api_run_request`（临时请求，不挂端点）与
//! `api_call_endpoint`（按 id 解析并发出端点）。
//!
//! 两者汇到同一条执行与结果封装路径，所以不论来源，输出形状一致。**两者也走同一个
//! 解析器**：`variable_fill::resolve_endpoint`。临时请求先被拼成一份内存里的
//! `EndpointDetail`，再交给它解析，故「变量替换 / base URL 拼接 / 鉴权派生 /
//! header 排序 / body 判空」这些规则只有一份实现 —— 不会因为「发临时请求」而另有一套
//! 拼接规则。
//!
//! 本文件只放登记、入参形状与两个 handler；入参的压平在 `input.rs`，结果信封在
//! `envelope.rs`。
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use crate::variable_fill::ResolvedEndpoint;
use simple_post_core::db::DbPool;
use simple_post_core::domain::{
    EndpointDetail, Environment, Project, RequestBody, RequestSettings,
};
use simple_post_core::error::AppError;

use self::envelope::build_result_envelope;
use self::input::{
    extract_endpoint_settings, extract_settings, flatten_key_values, format_method, parse_auth,
    parse_body_mode,
};

// =========================================================================
// Args 形状
// =========================================================================

#[derive(Debug, Deserialize)]
struct RunRequestArgs {
    method: String,
    url: String,
    /// 请求头。接受 `{"Foo": "bar"}`、`[["Foo","bar"]]`、
    /// `[{"key": "Foo", "value": "bar", "enabled": false}]` 三种形状。
    #[serde(default)]
    headers: Option<Value>,
    /// 查询参数。形状与请求头相同。
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    body: Option<String>,
    /// body 的模式（`none` / `json` / `form` / `raw` / `multipart`）。
    /// 给了 body 而没给模式时算 `raw`。
    #[serde(default)]
    body_mode: Option<String>,
    /// 鉴权方案，形状与端点存的那份一致：
    /// `{"kind": "bearer", "bearer": "…"}` / `{"kind": "basic", "basic": {…}}` /
    /// `{"kind": "apikey", "apikey": {…}}` / `{"kind": "oauth2", "oauth2": {…}}`。
    #[serde(default)]
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
                "Run an ad-hoc HTTP request. The request is assembled by the same resolver the stored endpoints use: env variables, baseUrl, auth, params and body all follow the endpoint rules. A base URL is read from moduleBaseUrls[base_url_definition_id][module_id], so both ids are needed to prepend one.",
            schema: json!({
                "type": "object",
                "properties": {
                    "method": { "type": "string" },
                    "url": { "type": "string" },
                    "headers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key", "value"]
                        }
                    },
                    "params": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key", "value"]
                        }
                    },
                    "body": { "type": "string" },
                    "body_mode": {
                        "type": "string",
                        "enum": ["none", "json", "form", "raw", "multipart"],
                        "description": "How to frame the body. Defaults to raw when a body is given, none otherwise. multipart takes the same JSON array of parts the endpoint body stores."
                    },
                    "auth": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["none", "bearer", "basic", "apikey", "oauth2"] },
                            "bearer": { "type": "string" },
                            "basic": {
                                "type": "object",
                                "properties": {
                                    "username": { "type": "string" },
                                    "password": { "type": "string" }
                                }
                            },
                            "apikey": {
                                "type": "object",
                                "properties": {
                                    "key": { "type": "string" },
                                    "value": { "type": "string" },
                                    "inLocation": { "type": "string", "enum": ["header", "query"] }
                                }
                            },
                            "oauth2": { "type": "object" }
                        },
                        "required": ["kind"]
                    },
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

    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    let headers = flatten_key_values(a.headers.as_ref(), "headers")?;
    let params = flatten_key_values(a.params.as_ref(), "params")?;
    let auth = parse_auth(a.auth.as_ref())?;
    let content = a.body.clone().unwrap_or_default();
    let body_mode = parse_body_mode(a.body_mode.as_deref(), !content.is_empty())?;

    // 环境：给了 id 就用它。取不到（环境已被删）时降级成「这次请求不做变量替换」。
    let env: Option<Environment> = match a.env_id.as_deref() {
        Some(env_id) => Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        })?),
        None => None,
    };

    // 项目：base URL 的取值住在 (环境 × 服务 × 模块) 交叉的格子里，而服务属于项目，
    // 故要知道项目才能读那张表。给了服务 id 就按它找，只给了环境就按环境找。
    let project = project_for(
        &state.pool,
        a.base_url_definition_id.as_deref(),
        env.as_ref(),
    )?;

    // 临时请求没有 `nodes` 行，故 `node_id` 是占位值 —— 解析器不用它。
    let detail = EndpointDetail {
        node_id: String::new(),
        method: method.clone(),
        url: a.url.clone(),
        environment_id: a.env_id.clone(),
        base_url_definition_id: a.base_url_definition_id.clone(),
        base_url_override: None,
        params,
        headers,
        body: RequestBody {
            mode: body_mode,
            content,
            schema: None,
        },
        auth,
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    };

    let overrides = crate::variable_fill::EndpointOverrides {
        module_id: a.module_id.clone(),
        ..Default::default()
    };
    let resolved =
        crate::variable_fill::resolve_endpoint(&detail, env.as_ref(), &project, Some(&overrides))?;

    let settings = resolve_settings(
        &state.pool,
        extract_settings(&a.settings_override, &a),
        a.settings_override.as_ref(),
    );

    send_resolved(
        state,
        &format_method(&method),
        resolved,
        env.as_ref(),
        None,
        settings,
    )
    .await
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

    // 环境：端点自己绑定的优先；没绑定就取项目当前激活的那个 —— 与界面那侧一致
    // （界面用的是工作区当前环境，而工作区的切换会写 `environments.is_active`）。
    let env =
        crate::mcp::resolve_env(&state.pool, endpoint.environment_id.as_deref(), &project_id)?;

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

    let project_ref: &Project = project.as_ref().unwrap_or(&EMPTY_PROJECT);
    let resolved = crate::variable_fill::resolve_endpoint(
        &endpoint,
        env.as_ref(),
        project_ref,
        Some(&overrides),
    )?;

    let settings = resolve_settings(
        &state.pool,
        extract_endpoint_settings(&a.settings_override),
        a.settings_override.as_ref(),
    );

    send_resolved(
        state,
        &format_method(&endpoint.method),
        resolved,
        env.as_ref(),
        Some(endpoint.node_id.clone()),
        settings,
    )
    .await
}

// =========================================================================
// 两个工具共用的发送路径
// =========================================================================

/// 一份已解析好的请求 → 发出 → 结果信封。
///
/// 两个发送工具共用它，故「解析之后发生什么」只有一份实现：oauth2 的 token 注入、
/// multipart 的装配、脚本链、发送、结果封装。
///
/// `endpoint_id` 决定脚本链能不能装配（有它才有 collection 那几段）；临时请求没有
/// 端点，故传 `None` —— 它的请求里没有脚本可跑。
async fn send_resolved(
    state: &AppState,
    method: &str,
    resolved: ResolvedEndpoint,
    env: Option<&Environment>,
    endpoint_id: Option<String>,
    settings: RequestSettings,
) -> Result<CallToolResult, AppError> {
    let ResolvedEndpoint {
        url,
        headers,
        body,
        body_mode,
        auth,
        missing_vars,
        ..
    } = resolved;
    let mut url = url;
    let mut headers = headers;

    // oauth2 的 token 是发请求之前才取到的，`resolve_endpoint`（纯函数）拿不到它，
    // 故注入放在这里而不是那里——`build_headers` 对 bearer/basic/apikey 的处理不受影响。
    //
    // **必须同时匹配 `kind`**：`resolve_auth` 对未选中的方案原样保留载荷，所以一个从
    // oauth2 切到 bearer 的端点仍带着 `oauth2: Some(..)`。只匹配载荷时 header 档碰巧
    // 被 `build_headers` 已写好的 `Authorization` 挡住，而 query 档**不挡**——一个用户
    // 已经不再选择的 token 会被拼进 URL 发出去。
    if let simple_post_core::domain::AuthConfig {
        kind: simple_post_core::domain::AuthType::Oauth2,
        oauth2: Some(cfg),
        ..
    } = &auth
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
    let mut body = body;
    if body_mode == simple_post_core::domain::BodyMode::Multipart {
        body = body
            .take()
            .map(|content| multipart::apply(&mut headers, &content));
    }

    // 脚本链跑在同一条序列里：前置脚本改写请求 → 发出 → 测试脚本断言并写回变量。
    // 整条序列由 core 一处实现，故桌面端与 MCP 端不会分叉。
    let outcome = simple_post_core::script::send_with_scripts(
        &state.pool,
        simple_post_core::script::ScriptedSendInput {
            endpoint_id,
            request: simple_post_core::script::ScriptRequest {
                method: method.to_string(),
                url: url.clone(),
                headers: headers.clone(),
                body,
            },
            env: env_snapshot(env),
            settings,
            // MCP 端没有「界面上未保存的草稿」这回事，用的就是存的那份脚本。
            script_override: None,
        },
    )
    .await?;

    state.metrics.inc();
    Ok(build_result_envelope(
        method,
        &outcome.request.url,
        &outcome.http,
        None,
        Some(missing_vars),
        outcome.script,
    ))
}

/// 脚本看得到的环境。**与解析用的是同一套变量池**：停用的变量不参与，脚本看到的
/// 环境与这次请求看到的是同一个。
fn env_snapshot(env: Option<&Environment>) -> simple_post_core::script::EnvSnapshot {
    let variables = env
        .map(|e| {
            e.variables
                .iter()
                .filter(|(k, _)| !e.disabled_variables.iter().any(|d| d == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    simple_post_core::script::EnvSnapshot {
        id: env.map(|e| e.id.clone()),
        variables,
    }
}

/// 本次发送用的出站旋钮（超时 / 跟随重定向 / 最大响应体积）。
///
/// 起点是**用户配置的那一份**（`preferences` 的 `request_settings` 行，桌面端在设置
/// 面板里改的就是它），调用方的覆盖值再盖上去。故界面上把超时调成 5 秒，MCP 侧也是
/// 5 秒；行缺失时用 `RequestSettings::default()`。
fn resolve_settings(
    pool: &DbPool,
    flat: (Option<u64>, Option<bool>, Option<u64>),
    override_: Option<&Value>,
) -> RequestSettings {
    let mut settings = simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::preferences::request_settings(c)
    })
    .unwrap_or_default();
    if let Some(t) = flat.0 {
        settings.timeout_ms = t;
    }
    if let Some(r) = flat.1 {
        settings.follow_redirects = r;
    }
    if let Some(m) = flat.2 {
        settings.max_response_size_kb = m;
    }
    if let Some(v) = override_ {
        if let Some(n) = v.get("timeout_ms").and_then(Value::as_u64) {
            settings.timeout_ms = n;
        }
        if let Some(b) = v.get("follow_redirects").and_then(Value::as_bool) {
            settings.follow_redirects = b;
        }
        if let Some(n) = v.get("max_response_size_kb").and_then(Value::as_u64) {
            settings.max_response_size_kb = n;
        }
    }
    settings
}

/// 临时请求要用的项目行。
///
/// base URL 的取值住在 (环境 × 服务 × 模块) 交叉的格子里，而服务属于项目，故要知道
/// 项目才能读那张表。给了服务 id 就按它找，只给了环境就按环境找；都没有就用空壳
/// （解析出空 base URL，不猜）。
fn project_for(
    pool: &DbPool,
    def_id: Option<&str>,
    env: Option<&Environment>,
) -> Result<Project, AppError> {
    let project_id = match def_id {
        Some(def_id) => Some(simple_post_core::db::with_conn(pool, |c| {
            simple_post_core::db::base_url_defs::project_id_of(c, def_id)
        })?),
        None => env.map(|e| e.project_id.clone()),
    };
    let Some(project_id) = project_id.filter(|id| !id.is_empty()) else {
        return Ok(EMPTY_PROJECT.clone());
    };
    Ok(simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::projects::get(c, &project_id)
    })
    .unwrap_or_else(|_| EMPTY_PROJECT.clone()))
}

// =========================================================================
// Project 占位回退
// =========================================================================

// `resolve_endpoint` 要一个 `&Project`，而项目不一定查得到；查不到就用这个空壳，
// 免得解析这一步去 borrow 一个 `None`。
// 必须是 `static` 而非 `const`：`&EMPTY_PROJECT` 要拿到的是 `&'static Project`，
// 引用 `const` 会产生语句结束即释放的临时值，存活的 `&Project` 会触发 E0716。
static EMPTY_PROJECT: Project = Project {
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
#[path = "http_ops/multipart_parity_test.rs"]
mod multipart_parity_tests;

#[cfg(test)]
#[path = "http_ops/run_request_test.rs"]
mod run_request_tests;

#[cfg(test)]
#[path = "http_ops/env_fallback_test.rs"]
mod env_fallback_tests;

#[cfg(test)]
#[path = "http_ops/script_test.rs"]
mod script_tests;
