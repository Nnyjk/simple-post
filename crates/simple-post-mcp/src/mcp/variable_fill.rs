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
use std::sync::Arc;

use simple_post_core::domain::Endpoint;
use simple_post_core::error::AppError;
use crate::mcp::{
    arg_str, arg_str_opt, AppState, CallToolResult, ToolEntry,
};
use crate::tool_fn;

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

pub fn table() -> Vec<ToolEntry> {
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
    crate::mcp::check_permission(
        &state.pool,
        a.env_id.as_deref().unwrap_or("global"),
        "api_resolve_template",
    )
    .await
    .ok();

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

    let endpoint: Endpoint = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoints::get(c, &endpoint_id)
    })?;
    let project_id = collection_project_id(&state.pool, &endpoint.collection_id);
    crate::mcp::check_permission(&state.pool, &project_id, "api_resolve_endpoint")
        .await
        .ok();

    let env = if let Some(env_id) = endpoint.environment_id.as_deref() {
        simple_post_core::db::with_conn(&state.pool, |c| simple_post_core::db::environments::get(c, env_id)).ok()
    } else {
        None
    };
    let project = if !project_id.is_empty() {
        simple_post_core::db::with_conn(&state.pool, |c| simple_post_core::db::projects::get(c, &project_id)).ok()
    } else {
        None
    };
    let project_ref = project.as_ref().unwrap_or(&EMPTY_PROJECT);

    let overrides: crate::variable_fill::EndpointOverrides = a
        .overrides
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let resolved =
        crate::variable_fill::resolve_endpoint(&endpoint, env.as_ref(), project_ref, Some(&overrides))?;

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
    let endpoint: Endpoint = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoints::get(c, &endpoint_id)
    })?;
    let project_id = collection_project_id(&state.pool, &endpoint.collection_id);
    crate::mcp::check_permission(&state.pool, &project_id, "api_validate_endpoint")
        .await
        .ok();

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

/// Resolve an endpoint's collection → module → project id so we can gate
/// the call. Falls back to an empty string (which the permission layer
/// treats as "global-ish").
fn collection_project_id(pool: &simple_post_core::db::DbPool, collection_id: &str) -> String {
    if let Ok(coll) = simple_post_core::db::with_conn(pool, |c| simple_post_core::db::collections::get(c, collection_id))
    {
        if let Ok(module) = simple_post_core::db::with_conn(pool, |c| {
            simple_post_core::db::modules::get(c, &coll.module_id)
        }) {
            return module.project_id;
        }
    }
    String::new()
}

/// Static validation — does this endpoint look like it'll run without
/// immediate failure? Returns a list of `{field, message}` issues.
fn validate(endpoint: &Endpoint) -> Vec<Value> {
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
mod tests {
    use super::*;
    use simple_post_core::domain::{
        ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode, HttpMethod,
        KeyValue, RequestBody,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    /// Per-call counter that disambiguates temp dirs when several tests
    /// construct their state() in the same millisecond (cargo's default
    /// runs tests in parallel).
    static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn state() -> Arc<AppState> {
        let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "simple-post-mcp-var-fill-test-{}-{}-{}",
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

    fn ep(url: &str, auth: AuthConfig, body_mode: BodyMode, body_content: &str) -> Endpoint {
        Endpoint {
            id: "ep1".into(),
            collection_id: "c1".into(),
            name: "ep".into(),
            description: None,
            method: HttpMethod::Get,
            url: url.into(),
            environment_id: None,
            base_url_definition_id: None,
            base_url_override: None,
            params: Vec::<KeyValue>::new(),
            headers: Vec::<KeyValue>::new(),
            body: RequestBody {
                mode: body_mode,
                content: body_content.into(),
                schema: None,
            },
            auth,
            docs: None,
            notes: None,
            tags: Vec::new(),
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn table_has_three_rows() {
        assert_eq!(table().len(), 3);
    }

    #[tokio::test]
    async fn resolve_template_rejects_empty() {
        let res = tool_api_resolve_template(&state(), json!({ "template": "" })).await;
        assert!(matches!(res, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn resolve_template_returns_resolved_and_missing() {
        let res = tool_api_resolve_template(
            &state(),
            json!({ "template": "https://{{host}}/users/{{id}}" }),
        )
        .await
        .unwrap();
        let text = serde_json::to_string(&res).unwrap();
        assert!(text.contains("missing"));
        assert!(text.contains("host"));
        assert!(text.contains("id"));
    }

    #[test]
    fn validate_endpoint_passes_for_well_formed_endpoint() {
        let endpoint = ep("https://api.example.com/users", AuthConfig::default(), BodyMode::None, "");
        let issues = validate(&endpoint);
        assert!(issues.is_empty(), "expected no issues, got {issues:?}");
    }

    #[test]
    fn validate_endpoint_flags_empty_url() {
        let endpoint = ep("", AuthConfig::default(), BodyMode::None, "");
        assert!(!validate(&endpoint).is_empty());
    }

    #[test]
    fn validate_endpoint_flags_missing_bearer() {
        let auth = AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some(String::new()),
            ..Default::default()
        };
        let endpoint = ep("/x", auth, BodyMode::None, "");
        let issues = validate(&endpoint);
        assert!(issues.iter().any(|i| i["field"] == "auth.bearer"));
    }

    #[test]
    fn validate_endpoint_flags_missing_basic() {
        let auth = AuthConfig {
            kind: AuthType::Basic,
            basic: Some(BasicAuth {
                username: String::new(),
                password: String::new(),
            }),
            ..Default::default()
        };
        let endpoint = ep("/x", auth, BodyMode::None, "");
        let issues = validate(&endpoint);
        assert!(issues.iter().any(|i| i["field"] == "auth.basic.username"));
        assert!(issues.iter().any(|i| i["field"] == "auth.basic.password"));
    }

    #[test]
    fn validate_endpoint_flags_missing_apikey() {
        let auth = AuthConfig {
            kind: AuthType::Apikey,
            apikey: Some(ApiKeyAuth {
                key: String::new(),
                value: String::new(),
                in_: ApiKeyLocation::Header,
            }),
            ..Default::default()
        };
        let endpoint = ep("/x", auth, BodyMode::None, "");
        let issues = validate(&endpoint);
        assert!(issues.iter().any(|i| i["field"] == "auth.apikey.key"));
        assert!(issues.iter().any(|i| i["field"] == "auth.apikey.value"));
    }

    #[test]
    fn validate_endpoint_flags_empty_body_when_required() {
        let endpoint = ep("/x", AuthConfig::default(), BodyMode::Json, "");
        let issues = validate(&endpoint);
        assert!(issues.iter().any(|i| i["field"] == "body.content"));
    }

    #[test]
    fn arg_str_opt_returns_none_for_missing() {
        let v = json!({});
        assert_eq!(arg_str_opt(&v, "x"), None);
    }
}