//! `endpoint_*` MCP tools — endpoint CRUD, search, duplicate, and movement.
//!
//! 8 tools:
//! - `endpoint_list`
//! - `endpoint_get`
//! - `endpoint_search`
//! - `endpoint_create`
//! - `endpoint_update`
//! - `endpoint_delete`
//! - `endpoint_duplicate`
//! - `endpoint_move`
//!
//! Private helpers also live here because they're only used by these
//! tools:
//! - `parse_body_mode`, `build_auth`: input parsing for `endpoint_create`.
//! - Permission gating uses the shared `crate::db::endpoints::project_id_of`
//!   helper to walk `collection → module → project`.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::{Endpoint, EndpointPatch, RequestBody};
use crate::error::AppError;
use crate::mcp::{arg_str, arg_str_opt, AppState, CallToolResult};
use crate::tool_fn;

#[derive(Debug, Deserialize)]
pub(super) struct EndpointCreateArgs {
    pub collection_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Option<Vec<crate::domain::KeyValue>>,
    #[serde(default)]
    pub params: Option<Vec<crate::domain::KeyValue>>,
    #[serde(default)]
    pub body_mode: Option<String>,
    #[serde(default)]
    pub body_content: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointMoveArgs {
    pub endpoint_id: String,
    pub new_collection_id: String,
    #[serde(default)]
    pub new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointSearchArgs {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub module_id: Option<String>,
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointGetArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDeleteArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDuplicateArgs {
    pub endpoint_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_list",
        description: "List endpoints in a project / module / collection.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "module_id": { "type": "string" },
                "collection_id": { "type": "string" }
            },
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_get",
        description: "Fetch a single endpoint by id.",
        schema: json!({
            "type": "object",
            "properties": { "endpoint_id": { "type": "string" } },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_get),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_search",
        description: "Search endpoints by free-text query, method, or tags.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "module_id": { "type": "string" },
                "collection_id": { "type": "string" },
                "q": { "type": "string" },
                "method": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_search),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_create",
        description: "Create a new endpoint inside a collection.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "name": { "type": "string" },
                "method": { "type": "string" },
                "url": { "type": "string" },
                "headers": { "type": "array", "items": { "type": "object" } },
                "params": { "type": "array", "items": { "type": "object" } },
                "body_mode": { "type": "string" },
                "body_content": { "type": "string" },
                "auth_type": { "type": "string" },
                "auth_config": { "type": "object" },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["collection_id", "name", "method", "url"],
            "additionalProperties": true,
        }),
        func: tool_fn!(tool_endpoint_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_update",
        description: "Patch an endpoint's fields.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "patch": { "type": "object" }
            },
            "required": ["endpoint_id", "patch"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_delete",
        description: "Delete an endpoint by id.",
        schema: json!({
            "type": "object",
            "properties": { "endpoint_id": { "type": "string" } },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_duplicate",
        description: "Duplicate an endpoint (optionally with a new name).",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_duplicate),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_move",
        description: "Move an endpoint to a new collection / position.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "new_collection_id": { "type": "string" },
                "new_sort_order": { "type": "integer" }
            },
            "required": ["endpoint_id", "new_collection_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_move),
    });
}

pub(super) async fn tool_endpoint_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str_opt(&args, "collection_id");
    let project_id = arg_str_opt(&args, "project_id");
    let _ = arg_str_opt(&args, "module_id"); // accepted but not used in this stub path

    crate::mcp::check_permission(
        &state.pool,
        project_id.as_deref().unwrap_or("global"),
        "endpoint_list",
    )
    .await?;

    if let Some(cid) = collection_id.as_deref() {
        let eps = crate::db::with_conn(&state.pool, |c| {
            crate::db::endpoints::list_by_collection(c, cid)
        })?;
        return Ok(CallToolResult::json(eps));
    }

    // No collection_id → fall back to the project-wide search (no
    // filter). The DB layer doesn't ship a "list all endpoints for a
    // project" function; search with all filters blank does the same
    // thing in practice.
    let eps = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::search(c, None, None, None)
    })?;
    Ok(CallToolResult::json(eps))
}

pub(super) async fn tool_endpoint_get(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointGetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_get args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = crate::db::endpoints::project_id_of(&state.pool, &ep.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_get").await?;
    Ok(CallToolResult::json(ep))
}

pub(super) async fn tool_endpoint_search(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointSearchArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_search args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        a.project_id.as_deref().unwrap_or("global"),
        "endpoint_list",
    )
    .await?;
    let tags_ref = a.tags.as_deref();
    let eps = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::search(c, a.q.as_deref(), a.method.as_deref(), tags_ref)
    })?;
    Ok(CallToolResult::json(eps))
}

pub(super) async fn tool_endpoint_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }
    // Validate the HTTP verb *before* touching the permission table:
    // input-level errors are cheap and shouldn't masquerade as
    // permission denials.
    let method = crate::domain::parse_http_method(&a.method)?;
    crate::mcp::check_permission(&state.pool, "global", "endpoint_create").await?;

    let body_mode_str = a.body_mode.clone().unwrap_or_else(|| "none".into());
    let body_mode = parse_body_mode(&body_mode_str)?;
    let body = RequestBody {
        mode: body_mode,
        content: a.body_content.clone().unwrap_or_default(),
        schema: None,
    };
    let auth = build_auth(a.auth_type.as_deref(), a.auth_config.clone())?;

    let now = crate::domain::now_ms();
    let ep = Endpoint {
        id: format!("ep_{now}"),
        collection_id: a.collection_id,
        name: a.name,
        description: None,
        method,
        url: a.url,
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: a.params.unwrap_or_default(),
        headers: a.headers.unwrap_or_default(),
        body,
        auth,
        docs: None,
        notes: None,
        tags: a.tags.unwrap_or_default(),
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let id = ep.id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::endpoints::create(c, &ep))?;
    let stored = crate::db::with_conn(&state.pool, |c| crate::db::endpoints::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_endpoint_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let endpoint_id = arg_str(&args, "endpoint_id")?.to_string();
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &endpoint_id)
    })?;
    let project_id = crate::db::endpoints::project_id_of(&state.pool, &ep.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_update").await?;

    let patch_obj = args.get("patch").cloned().unwrap_or(Value::Null);
    let patch = EndpointPatch {
        name: patch_obj.get("name").and_then(Value::as_str).map(String::from),
        description: patch_obj
            .get("description")
            .map(|v| v.as_str().map(String::from)),
        method: patch_obj
            .get("method")
            .and_then(Value::as_str)
            .and_then(|m| crate::domain::parse_http_method(m).ok()),
        url: patch_obj.get("url").and_then(Value::as_str).map(String::from),
        environment_id: patch_obj
            .get("environment_id")
            .map(|v| v.as_str().map(String::from)),
        base_url_definition_id: patch_obj
            .get("base_url_definition_id")
            .map(|v| v.as_str().map(String::from)),
        base_url_override: patch_obj
            .get("base_url_override")
            .map(|v| v.as_str().map(String::from)),
        params: patch_obj.get("params").and_then(|v| serde_json::from_value(v.clone()).ok()),
        headers: patch_obj.get("headers").and_then(|v| serde_json::from_value(v.clone()).ok()),
        body: patch_obj.get("body").and_then(|v| serde_json::from_value(v.clone()).ok()),
        auth: patch_obj.get("auth").and_then(|v| serde_json::from_value(v.clone()).ok()),
        docs: None,
        notes: None,
        tags: patch_obj
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect()),
        sort_order: patch_obj.get("sort_order").and_then(Value::as_i64),
    };
    let updated = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::update(c, &endpoint_id, patch)
    })?;
    Ok(CallToolResult::json(updated))
}

pub(super) async fn tool_endpoint_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_delete args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = crate::db::endpoints::project_id_of(&state.pool, &ep.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_delete").await?;
    crate::db::with_tx(&state.pool, |c| {
        crate::db::endpoints::delete(c, &a.endpoint_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.endpoint_id })))
}

pub(super) async fn tool_endpoint_duplicate(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDuplicateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_duplicate args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = crate::db::endpoints::project_id_of(&state.pool, &ep.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_create").await?;

    let now = crate::domain::now_ms();
    let mut dup = ep.clone();
    dup.id = format!("ep_{now}");
    dup.name = a.name.unwrap_or_else(|| ep.name.clone());
    dup.created_at = now;
    dup.updated_at = now;
    let id = dup.id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::endpoints::create(c, &dup))?;
    let stored = crate::db::with_conn(&state.pool, |c| crate::db::endpoints::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_endpoint_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_move args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = crate::db::endpoints::project_id_of(&state.pool, &ep.collection_id)?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_update").await?;

    // The DB layer's `endpoints::update` doesn't accept a new
    // collection_id / sort_order in EndpointPatch (only domain-field
    // patches). Implement move as delete + create under the new
    // collection so we don't fight the FK cascade in the wrong
    // direction.
    let mut moved = ep.clone();
    moved.id = format!("ep_{}", crate::domain::now_ms());
    moved.collection_id = a.new_collection_id.clone();
    moved.sort_order = a.new_sort_order.unwrap_or(0);
    let now = crate::domain::now_ms();
    moved.created_at = now;
    moved.updated_at = now;
    let new_id = moved.id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::endpoints::delete(c, &a.endpoint_id))?;
    crate::db::with_tx(&state.pool, |c| crate::db::endpoints::create(c, &moved))?;
    Ok(CallToolResult::json(json!({
        "endpoint_id": new_id,
        "new_collection_id": a.new_collection_id,
    })))
}

// =========================================================================
// Private helpers (only used by the endpoint_* tools above)
// =========================================================================

fn parse_body_mode(s: &str) -> Result<crate::domain::BodyMode, AppError> {
    use crate::domain::BodyMode;
    Ok(match s.to_ascii_lowercase().as_str() {
        "none" => BodyMode::None,
        "json" => BodyMode::Json,
        "form" => BodyMode::Form,
        "raw" => BodyMode::Raw,
        other => return Err(AppError::Validation(format!("unknown body_mode: {other}"))),
    })
}

fn build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<crate::domain::AuthConfig, AppError> {
    use crate::domain::{AuthConfig, AuthType};
    let kind = match auth_type.unwrap_or("none").to_ascii_lowercase().as_str() {
        "none" => AuthType::None,
        "bearer" => AuthType::Bearer,
        "basic" => AuthType::Basic,
        "apikey" => AuthType::Apikey,
        other => return Err(AppError::Validation(format!("unknown auth type: {other}"))),
    };
    let cfg = auth_config.unwrap_or(Value::Null);
    // Best-effort deserialize of auth_config. The TS side uses `{}` when
    // there's nothing to set, which is fine for any auth type.
    let mut out: AuthConfig = serde_json::from_value(cfg).unwrap_or_default();
    out.kind = kind;
    Ok(out)
}

// =========================================================================
// Test-only re-exports
// =========================================================================
//
// `mod.rs::tests` needs to call `parse_body_mode` and `build_auth` to
// verify their behaviour. They're file-private helpers above; expose
// them with `#[cfg(test)]` so production callers can't see them.

#[cfg(test)]
pub(super) fn __test_parse_body_mode(s: &str) -> Result<crate::domain::BodyMode, AppError> {
    parse_body_mode(s)
}

#[cfg(test)]
pub(super) fn __test_build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<crate::domain::AuthConfig, AppError> {
    build_auth(auth_type, auth_config)
}