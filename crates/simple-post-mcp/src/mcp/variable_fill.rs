//! 变量填充 / 校验类 MCP 工具 —— 共 3 个：`api_resolve_template`、
//! `api_resolve_endpoint`、`api_validate_endpoint`。
//!
//! 都是底层 `crate::variable_fill` 的 MCP 适配层：重活在那里干，这里补上权限检查、
//! 参数校验与 JSON-RPC 形状的返回值。
//!
//! 命名：工具名带 `api_*` 前缀，对齐 `src/lib/http.ts` 的 TS 侧惯例；Rust 模块名
//! 跟底层文件名一致。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::{EndpointDetail, OAuth2GrantType};
use simple_post_core::error::AppError;

// =========================================================================
// Args 形状
// =========================================================================

#[derive(Debug, Deserialize)]
struct ResolveTemplateArgs {
    template: String,
    #[serde(default)]
    env_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResolveEndpointArgs {
    endpoint_id: String,
    #[serde(default)]
    overrides: Option<Value>,
}

// =========================================================================
// dispatch 表
// =========================================================================

pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "api_resolve_template",
            description:
                "Resolve `{{var}}` markers in a free-form template string against an environment. Returns `{resolved, missing}`.",
            schema: json!({
                "type": "object",
                "properties": {
                    "template": { "type": "string" },
                    "env_id": { "type": "string" },
                    "scope": { "type": "string", "enum": ["request", "headers", "params", "body", "auth"] }
                },
                "required": ["template"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_api_resolve_template),
        },
        ToolEntry {
            name: "api_resolve_endpoint",
            description:
                "Resolve an endpoint's full request (method, url, headers, params, body, auth) against env + overrides. Returns `{resolved, missing}`.",
            schema: json!({
                "type": "object",
                "properties": {
                    "endpoint_id": { "type": "string" },
                    "overrides": { "type": "object" }
                },
                "required": ["endpoint_id"],
                "additionalProperties": true,
            }),
            func: tool_fn!(tool_api_resolve_endpoint),
        },
        ToolEntry {
            name: "api_validate_endpoint",
            description:
                "Static check that an endpoint is ready to use: the URL is non-empty, the auth fields are filled, the body is present when the mode needs one, and the payload is not a JSON Schema written into the payload slot. Returns `{ok, issues}` — `ok` is false when any issue was found; an issue means \"fix this\", not \"this endpoint cannot run\".",
            schema: json!({
                "type": "object",
                "properties": { "endpoint_id": { "type": "string" } },
                "required": ["endpoint_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_api_validate_endpoint),
        },
    ]
}

// =========================================================================
// 工具实现
// =========================================================================

async fn tool_api_resolve_template(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ResolveTemplateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_resolve_template args: {e}")))?;
    if a.template.is_empty() {
        return Err(AppError::Validation("template must not be empty".into()));
    }
    // 没有 `env_id` 就是纯模板替换，没有项目上下文：档位已由 Gate A 判过，没有
    // 按项目的行可查。给了 `env_id` 就按该环境所属项目过门。
    let gate_project = match a.env_id.as_deref() {
        Some(env_id) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::lookup_project_id(c, env_id)
        })
        .unwrap_or_default(),
        None => String::new(),
    };
    crate::mcp::check_permission(&state.pool, &gate_project, "api_resolve_template").await?;

    // 取指定环境的变量，取不到就用空表。
    let vars = if let Some(env_id) = a.env_id.as_deref() {
        simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        })
        .map(|e| e.variables)
        .unwrap_or_default()
    } else {
        Default::default()
    };

    let _ = a.scope; // 留给将来的分域替换（headers/body 等）
    let resolved = crate::variable_fill::resolve_template(&a.template, &vars);

    Ok(CallToolResult::json(json!({
        "resolved": resolved.resolved,
        "missing": resolved.missing,
    })))
}

async fn tool_api_resolve_endpoint(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ResolveEndpointArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_resolve_endpoint args: {e}")))?;
    let endpoint_id = a.endpoint_id.clone();

    let endpoint: EndpointDetail = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, &endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &endpoint.node_id);
    crate::mcp::check_permission(&state.pool, &project_id, "api_resolve_endpoint").await?;

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
    let project_ref = project.as_ref().unwrap_or(&EMPTY_PROJECT);

    let overrides: crate::variable_fill::EndpointOverrides = a
        .overrides
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // 与 `api_call_endpoint` 同一套按模块查找：预览与真正发送必须对「这个端点解析
    // 到哪个 host」达成一致，否则 AI 客户端预览一个、实际打到另一个。
    let mut overrides = overrides;
    if overrides.module_id.is_none() {
        overrides.module_id = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::module_id_of(c, &endpoint.node_id)
        })
        .ok()
        .flatten();
    }

    let resolved = crate::variable_fill::resolve_endpoint(
        &endpoint,
        env.as_ref(),
        project_ref,
        Some(&overrides),
    )?;

    Ok(CallToolResult::json(json!({
        "method": resolved.method,
        "url": resolved.url,
        "headers": resolved.headers,
        "params": resolved.params,
        "body": resolved.body,
        "auth": resolved.auth,
        "missing_vars": resolved.missing_vars,
        // 路径参数缺值时 `url` 里留着 `{id}` 这样的占位符。这一份清单告诉调用方
        // 缺的是哪个，发送那侧会因此拦下请求。
        "missing_path_params": resolved.missing_path_params,
    })))
}

async fn tool_api_validate_endpoint(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let endpoint_id = arg_str(&args, "endpoint_id")?.to_string();
    let endpoint: EndpointDetail = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, &endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &endpoint.node_id);
    crate::mcp::check_permission(&state.pool, &project_id, "api_validate_endpoint").await?;

    // 静态检查要按**拼接后**的 URL 找路径占位符：base URL 里也可能有 `{version}` 这样的
    // 占位符，而发送那侧是在拼接后的 URL 上做替换。只看端点自己的 URL，会让静态检查说
    // 「没问题」、真发出去却被拦下 —— 工具就在骗调用方。
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
    let project_ref = project.as_ref().unwrap_or(&EMPTY_PROJECT);
    // 与发送那侧同一套按模块查找：预览、检查与真正发送必须对「这个端点解析到哪个
    // host」达成一致。
    let module_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::module_id_of(c, &endpoint.node_id)
    })
    .ok()
    .flatten();
    let (base_url, _src, _def_id) = crate::variable_fill::resolve_base_url_with(
        &endpoint,
        env.as_ref(),
        project_ref,
        None,
        module_id.as_deref(),
    );
    let joined = crate::variable_fill::join_base_url(&base_url, &endpoint.url);

    let issues = validate(&endpoint, &joined);
    let ok = issues.is_empty();
    Ok(CallToolResult::json(json!({
        "ok": ok,
        "issues": issues,
    })))
}

// =========================================================================
// 本文件内的辅助函数
// =========================================================================

/// 端点 node → project id，供按项目的那扇门使用。
///
/// 查不到时退化成空串 —— 那等于「没有这一行」，取默认 `ReadCall`。本文件三个工具
/// 都是 `Read` 档，所以这层退化不会误放行写操作；将来若挂上更高档位的工具，空串
/// 就会把调用永久拒掉。
/// `root_id` 已反规范化在每一行上，一次索引读即可，不必回溯父链。
fn endpoint_project_id(pool: &simple_post_core::db::DbPool, node_id: &str) -> String {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
    .unwrap_or_default()
}

/// 静态检查：端点看起来跑不跑得起来。返回 `{field, message}` 形式的 issues 列表。
/// 静态检查这个端点。
///
/// `url` 是**拼接后**的 URL（base URL + 端点自己的路径），路径占位符从它里面取：
/// base URL 里也可能有 `{version}` 这样的占位符，而发送那侧是在拼接后的 URL 上做
/// 替换，只看端点自己的 URL 会漏掉它。
fn validate(endpoint: &EndpointDetail, url: &str) -> Vec<Value> {
    use simple_post_core::domain::{AuthType, BodyMode};
    let mut issues = Vec::new();

    if endpoint.url.trim().is_empty() {
        issues.push(json!({ "field": "url", "message": "url must not be empty" }));
    } else if !endpoint.url.starts_with("http://")
        && !endpoint.url.starts_with("https://")
        && !endpoint.url.starts_with("/")
        && !endpoint.url.starts_with("{{")
    {
        issues.push(json!({
            "field": "url",
            "message": "url should start with http(s):// or be a path or a template"
        }));
    }

    // 路径参数缺值：路径里留着 `{id}` 这样的占位符，而端点没有给它赋值。发送那侧
    // 会因此拦下请求，故静态检查在这里先点出来。
    for name in simple_post_core::path_params::placeholders(url) {
        let has_value = endpoint
            .path_params
            .iter()
            .any(|p| p.enabled && p.key == name && !p.value.trim().is_empty());
        if !has_value {
            issues.push(json!({
                "field": format!("pathParams.{name}"),
                "message": format!(
                    "path parameter '{name}' has no value; the url keeps the literal {{{name}}}"
                )
            }));
        }
    }

    match endpoint.auth.kind {
        AuthType::None => {}
        AuthType::Bearer => {
            if endpoint
                .auth
                .bearer
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                issues.push(json!({
                    "field": "auth.bearer",
                    "message": "bearer token required"
                }));
            }
        }
        AuthType::Basic => {
            let user_missing = endpoint
                .auth
                .basic
                .as_ref()
                .map(|b| b.username.is_empty())
                .unwrap_or(true);
            let pass_missing = endpoint
                .auth
                .basic
                .as_ref()
                .map(|b| b.password.is_empty())
                .unwrap_or(true);
            if user_missing {
                issues.push(json!({ "field": "auth.basic.username", "message": "required" }));
            }
            if pass_missing {
                issues.push(json!({ "field": "auth.basic.password", "message": "required" }));
            }
        }
        AuthType::Apikey => {
            let key_missing = endpoint
                .auth
                .apikey
                .as_ref()
                .map(|a| a.key.is_empty())
                .unwrap_or(true);
            let val_missing = endpoint
                .auth
                .apikey
                .as_ref()
                .map(|a| a.value.is_empty())
                .unwrap_or(true);
            if key_missing {
                issues.push(json!({ "field": "auth.apikey.key", "message": "required" }));
            }
            if val_missing {
                issues.push(json!({ "field": "auth.apikey.value", "message": "required" }));
            }
        }
        AuthType::Oauth2 => {
            // 缺载荷时只报这一条并继续走完其余检查（body 的检查在这个 match 之后）。
            // 其余鉴权分支也是「push 完继续」，早退会让 body 问题对 oauth2 端点凭空消失。
            if let Some(o) = endpoint.auth.oauth2.as_ref() {
                // 只报「缺哪个字段」，不代填也不拒收：真正的判定在取 token 时
                // (`oauth2::validate`)，那里才知道该流程的完整要求。
                let mut need = |field: &str, ok: bool| {
                    if !ok {
                        issues.push(json!({ "field": field, "message": "required" }));
                    }
                };
                match o.grant_type {
                    OAuth2GrantType::Implicit => {
                        need(
                            "auth.oauth2.authorizationUrl",
                            !o.authorization_url.trim().is_empty(),
                        );
                    }
                    OAuth2GrantType::ClientCredentials => {
                        need("auth.oauth2.tokenUrl", !o.token_url.trim().is_empty());
                        need("auth.oauth2.clientId", !o.client_id.trim().is_empty());
                    }
                    OAuth2GrantType::Password => {
                        need("auth.oauth2.tokenUrl", !o.token_url.trim().is_empty());
                        need("auth.oauth2.clientId", !o.client_id.trim().is_empty());
                        need("auth.oauth2.username", !o.username.trim().is_empty());
                    }
                    OAuth2GrantType::AuthorizationCode => {
                        need("auth.oauth2.tokenUrl", !o.token_url.trim().is_empty());
                        need("auth.oauth2.clientId", !o.client_id.trim().is_empty());
                        // 这里只看非空：粘进来的是一条回调 URL 时，真正的 `code` 要等
                        // `oauth2::extract_code` 才解析得出来，而那条路径在取 token 时。
                        // 提前报「缺授权码」只会把一条有效的粘贴判成无效。
                        need("auth.oauth2.authCode", !o.auth_code.trim().is_empty());
                    }
                }
            } else {
                issues.push(json!({
                    "field": "auth.oauth2",
                    "message": "auth_type=oauth2 requires an `oauth2` config object"
                }));
            }
        }
    }

    match endpoint.body.mode {
        BodyMode::None => {}
        BodyMode::Json | BodyMode::Form | BodyMode::Multipart | BodyMode::Raw => {
            if endpoint.body.content.trim().is_empty() {
                issues.push(json!({
                    "field": "body.content",
                    "message": format!("body_mode={:?} requires non-empty body_content", endpoint.body.mode)
                }));
            }
        }
    }

    // 载荷与定义写反了：body.content 看起来是一份 JSON Schema，而 body.schema 还是空的。
    // 与 `endpoint_create` / `endpoint_update` 那道门同一个判据，故三处对同一个端点
    // 的结论一致。
    if endpoint.body.payload_may_be_a_schema() {
        issues.push(json!({
            "field": "body.content",
            "message": "body.content looks like a JSON Schema, not a request payload. \
                        body.content is what is actually sent; the documented parameter \
                        definition belongs in body.schema."
        }));
    }

    issues
}

// 项目已被删除时的空壳，让调用方不必处理悬垂引用。
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
#[path = "variable_fill_test.rs"]
mod tests;
