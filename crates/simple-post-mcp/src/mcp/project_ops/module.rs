//! `module_*` MCP tools — module CRUD inside a project.
//!
//! 5 tools:
//! - `module_list`
//! - `module_create`
//! - `module_update`
//! - `module_delete`
//! - `module_move`
//!
//! `module_list` reuses `super::ProjectIdArgs` (a module is owned by a
//! project). The other Args structs are private to this module.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{Module, ModulePatch};
use simple_post_core::error::AppError;

#[derive(Debug, Deserialize)]
pub(super) struct ModuleCreateArgs {
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModuleUpdateArgs {
    pub module_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModuleMoveArgs {
    pub module_id: String,
    pub new_project_id: String,
    #[serde(default)]
    pub new_sort_order: Option<i64>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "module_list",
        description: "List modules belonging to a project.",
        schema: json!({
            "type": "object",
            "properties": { "project_id": { "type": "string" } },
            "required": ["project_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_module_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "module_create",
        description: "Create a module in a project.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["project_id", "name"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_module_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "module_update",
        description: "Rename a module.",
        schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_module_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "module_delete",
        description: "Delete a module.",
        schema: json!({
            "type": "object",
            "properties": { "module_id": { "type": "string" } },
            "required": ["module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_module_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "module_move",
        description: "Move a module to a different project + position.",
        schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string" },
                "new_project_id": { "type": "string" },
                "new_sort_order": { "type": "integer" }
            },
            "required": ["module_id", "new_project_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_module_move),
    });
}

pub(super) async fn tool_module_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: super::ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "module_list").await?;
    let mods = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::modules::list_by_project(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(mods))
}

pub(super) async fn tool_module_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ModuleCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, &a.project_id, "module_create").await?;
    let m = Module {
        id: format!("mod_{}", simple_post_core::domain::now_ms()),
        project_id: a.project_id.clone(),
        name: a.name,
        description: None,
        sort_order: 0,
        expanded: None,
    };
    let id = m.id.clone();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::modules::create(c, &m)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::modules::get(c, &id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_module_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ModuleUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "module_update").await?;
    let patch = ModulePatch {
        name: a.name.clone(),
        description: None,
        sort_order: None,
    };
    let m = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::modules::update(c, &a.module_id, patch)
    })?;
    Ok(CallToolResult::json(m))
}

pub(super) async fn tool_module_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let module_id = arg_str(&args, "module_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "module_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::modules::delete(c, &module_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": module_id })))
}

pub(super) async fn tool_module_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ModuleMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_move args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.new_project_id, "module_move").await?;

    // `modules::update` doesn't expose a `project_id` field on
    // ModulePatch — moving a module between projects isn't a first-class
    // operation. Implement as delete-then-create to keep the FK happy.
    let module = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::modules::get(c, &a.module_id)
    })?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::modules::delete(c, &a.module_id)
    })?;
    let new_id = format!("mod_{}", simple_post_core::domain::now_ms());
    let new_mod = Module {
        id: new_id.clone(),
        project_id: a.new_project_id.clone(),
        name: module.name,
        description: module.description,
        sort_order: a.new_sort_order.unwrap_or(0),
        expanded: None,
    };
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::modules::create(c, &new_mod)
    })?;
    Ok(CallToolResult::json(json!({
        "module_id": new_id,
        "new_project_id": a.new_project_id,
    })))
}
