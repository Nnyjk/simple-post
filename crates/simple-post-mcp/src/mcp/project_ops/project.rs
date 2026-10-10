//! `project_*` MCP tools — top-level project CRUD.
//!
//! 5 tools:
//! - `project_list`
//! - `project_get`
//! - `project_create`
//! - `project_update`
//! - `project_delete`
//!
//! Args structs that are only used here live alongside their tool impls.
//! Shared structs (`ProjectIdArgs`, `ModuleIdArgs`) stay in `super` and
//! are imported via `super::ProjectIdArgs`.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{Project, ProjectPatch};
use simple_post_core::error::AppError;

#[derive(Debug, Deserialize)]
pub(super) struct ProjectCreateArgs {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ProjectUpdateArgs {
    pub project_id: String,
    #[serde(default)]
    pub patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ProjectDeleteArgs {
    pub project_id: String,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "project_list",
        description: "List all projects.",
        schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        func: tool_fn!(tool_project_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "project_get",
        description: "Fetch a single project by id.",
        schema: json!({
            "type": "object",
            "properties": { "project_id": { "type": "string" } },
            "required": ["project_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_project_get),
    });
    v.push(crate::mcp::ToolEntry {
        name: "project_create",
        description: "Create a new project.",
        schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "color": { "type": "string" }
            },
            "required": ["name"],
            "additionalProperties": true,
        }),
        func: tool_fn!(tool_project_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "project_update",
        description: "Patch a project's fields.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "patch": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "color": { "type": "string" }
                    },
                    "additionalProperties": true
                }
            },
            "required": ["project_id", "patch"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_project_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "project_delete",
        description: "Delete a project by id.",
        schema: json!({
            "type": "object",
            "properties": { "project_id": { "type": "string" } },
            "required": ["project_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_project_delete),
    });
}

pub(super) async fn tool_project_list(
    state: &AppState,
    _args: Value,
) -> Result<CallToolResult, AppError> {
    // No per-project gate: listing projects spans every project, so there
    // is no single owning row to consult. Gate A (the global
    // `CapabilityPolicy`) is the gate that governs this tool.
    let projects =
        simple_post_core::db::with_conn(&state.pool, simple_post_core::db::projects::list)?;
    Ok(CallToolResult::json(projects))
}

pub(super) async fn tool_project_get(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: super::ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_get args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_get").await?;
    let p = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::projects::get(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(p))
}

pub(super) async fn tool_project_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ProjectCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    // No per-project gate here: the project does not exist yet, so there is
    // no owning project whose stored permission could apply. Gate A (the
    // global `CapabilityPolicy`, enforced in `mcp::dispatch`) is the gate
    // that governs this tool — it must be `Create` or higher.
    let now = simple_post_core::domain::now_ms();
    let p = Project {
        id: format!("proj_{now}"),
        name: a.name,
        description: a.description,
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: a.color.unwrap_or_else(|| "#3B82F6".into()),
        created_at: now,
        updated_at: now,
    };
    let id = p.id.clone();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::projects::create(c, &p)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::projects::get(c, &id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_project_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ProjectUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_update").await?;

    let patch = ProjectPatch {
        name: a
            .patch
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(String::from),
        description: a
            .patch
            .as_ref()
            .and_then(|p| p.get("description"))
            .map(|v| v.as_str().map(String::from)),
        base_url: None,
        base_url_definitions: None,
        default_base_url_definition_id: None,
        color: a
            .patch
            .as_ref()
            .and_then(|p| p.get("color"))
            .and_then(Value::as_str)
            .map(String::from),
    };
    let p = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::projects::update(c, &a.project_id, patch)
    })?;
    Ok(CallToolResult::json(p))
}

pub(super) async fn tool_project_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ProjectDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_delete args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::projects::delete(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.project_id })))
}
