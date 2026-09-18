//! `base_url_def_*` MCP tools — project-level baseUrl definition slots.
//!
//! 5 tools:
//! - `base_url_def_list`
//! - `base_url_def_create`
//! - `base_url_def_update`
//! - `base_url_def_delete`
//! - `base_url_def_set_default`
//!
//! `base_url_def_list` reuses `super::ProjectIdArgs`. The other Args
//! structs are local to this module.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::BaseUrlDefinition;
use crate::error::AppError;
use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;

#[derive(Debug, Deserialize)]
pub(super) struct BaseUrlDefCreateArgs {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BaseUrlDefUpdateArgs {
    pub definition_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BaseUrlDefDeleteArgs {
    pub definition_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct BaseUrlDefSetDefaultArgs {
    pub project_id: String,
    pub definition_id: String,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_list",
        description: "List project-level baseUrl definitions.",
        schema: json!({
            "type": "object",
            "properties": { "project_id": { "type": "string" } },
            "required": ["project_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_base_url_def_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_create",
        description: "Create a baseUrl definition (a named slot in the project).",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" }
            },
            "required": ["project_id", "name"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_base_url_def_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_update",
        description: "Rename a baseUrl definition.",
        schema: json!({
            "type": "object",
            "properties": {
                "definition_id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": ["string", "null"] }
            },
            "required": ["definition_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_base_url_def_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_delete",
        description: "Delete a baseUrl definition.",
        schema: json!({
            "type": "object",
            "properties": { "definition_id": { "type": "string" } },
            "required": ["definition_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_base_url_def_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_set_default",
        description: "Mark a baseUrl definition as the project's default.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "definition_id": { "type": "string" }
            },
            "required": ["project_id", "definition_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_base_url_def_set_default),
    });
}

pub(super) async fn tool_base_url_def_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: super::ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_list").await?;
    let defs = crate::db::with_conn(&state.pool, |c| {
        crate::db::base_url_defs::list_by_project(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(defs))
}

pub(super) async fn tool_base_url_def_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_create").await?;
    let def = BaseUrlDefinition {
        id: format!("def_{}", crate::domain::now_ms()),
        name: a.name,
        description: a.description,
    };
    let id = def.id.clone();
    let pid = a.project_id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::base_url_defs::create(c, &def, &pid))?;
    let stored = crate::db::with_conn(&state.pool, |c| {
        crate::db::base_url_defs::get(c, &id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_base_url_def_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "base_url_def_update").await?;
    let desc = a
        .description
        .as_ref()
        .map(|d| d.as_ref().map(String::from));
    let d = crate::db::with_conn(&state.pool, |c| {
        crate::db::base_url_defs::update(c, &a.definition_id, a.name.clone(), desc)
    })?;
    Ok(CallToolResult::json(d))
}

pub(super) async fn tool_base_url_def_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_delete args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "base_url_def_delete").await?;
    crate::db::with_tx(&state.pool, |c| {
        crate::db::base_url_defs::delete(c, &a.definition_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.definition_id })))
}

pub(super) async fn tool_base_url_def_set_default(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefSetDefaultArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_set_default args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_update").await?;
    crate::db::with_tx(&state.pool, |c| {
        crate::db::base_url_defs::set_default(c, &a.project_id, Some(&a.definition_id))
    })?;
    Ok(CallToolResult::json(json!({
        "project_id": a.project_id,
        "default_definition_id": a.definition_id,
    })))
}