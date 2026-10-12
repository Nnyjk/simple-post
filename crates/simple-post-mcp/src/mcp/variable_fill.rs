//! Variable-fill / validation MCP tools — 3 tools total.
//!
//! | tool                     | role                                              |
//! |--------------------------|---------------------------------------------------|
//! | api_resolve_template     | Resolve `{{var}}` markers in a free-form string   |
//! | api_resolve_endpoint     | Resolve an endpoint's full request                |
//! | api_validate_endpoint    | Static check that an endpoint looks runnable      |
//!
//! These are MCP adapters over the base `crate::variable_fill` module. The
//! base layer does the heavy lifting; we wrap with permission checks,
//! argument validation, and JSON-RPC-shaped return types.
//!
//! ## Naming
//!
//! MCP tool names use the `api_*` prefix to match the TS-side convention
//! in `src/lib/http.ts`. The Rust module is `variable_fill` to align with
//! the base layer's file name.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::EndpointDetail;
use simple_post_core::error::AppError;

// =========================================================================
// Args shapes
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
// Dispatch table
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
                "Static check that an endpoint is runnable (URL non-empty, auth fields filled, etc). Returns `{ok, issues}`.",
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
// Tool impls
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
    // No `env_id` means a pure template resolution with no project context;
    // Gate A already covers the tool's tier, so there is no per-project row
    // to consult. With an `env_id`, gate against that environment's project.
    let gate_project = match a.env_id.as_deref() {
        Some(env_id) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::lookup_project_id(c, env_id)
        })
        .unwrap_or_default(),
        None => String::new(),
    };
    crate::mcp::check_permission(&state.pool, &gate_project, "api_resolve_template").await?;

    // Pull variables from the named env (if any), else empty.
    let vars = if let Some(env_id) = a.env_id.as_deref() {
        simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        })
        .map(|e| e.variables)
        .unwrap_or_default()
    } else {
        Default::default()
    };

    let _ = a.scope; // reserved for future scoping (headers/body etc.)
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

    let env = if let Some(env_id) = endpoint.environment_id.as_deref() {
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
    let project_ref = project.as_ref().unwrap_or(&EMPTY_PROJECT);

    let overrides: crate::variable_fill::EndpointOverrides = a
        .overrides
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Same per-module lookup as `api_call_endpoint`: `api_resolve_endpoint`
    // has to agree with the send path about which URL an endpoint resolves
    // to, or an AI client previews one host and the send goes to another.
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

    let issues = validate(&endpoint);
    let ok = issues.is_empty();
    Ok(CallToolResult::json(json!({
        "ok": ok,
        "issues": issues,
    })))
}

// =========================================================================
// Local helpers
// =========================================================================

/// Resolve an endpoint node → project id so we can gate the call.
/// Falls back to an empty string (which the permission layer treats as
/// "global-ish"). One indexed lookup: `root_id` is denormalised onto
/// every node precisely so this does not have to walk the parent chain.
fn endpoint_project_id(pool: &simple_post_core::db::DbPool, node_id: &str) -> String {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
    .unwrap_or_default()
}

/// Static validation — does this endpoint look like it'll run without
/// immediate failure? Returns a list of `{field, message}` issues.
fn validate(endpoint: &EndpointDetail) -> Vec<Value> {
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
    }

    match endpoint.body.mode {
        BodyMode::None => {}
        BodyMode::Json | BodyMode::Form | BodyMode::Raw => {
            if endpoint.body.content.trim().is_empty() {
                issues.push(json!({
                    "field": "body.content",
                    "message": format!("body_mode={:?} requires non-empty body_content", endpoint.body.mode)
                }));
            }
        }
    }

    issues
}

/// Placeholder used when an endpoint's project has been deleted. The
/// variable_fill resolver requires a `&Project`; this keeps callers
/// alive in dangling-reference scenarios.
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
#[path = "variable_fill_test.rs"]
mod tests;
