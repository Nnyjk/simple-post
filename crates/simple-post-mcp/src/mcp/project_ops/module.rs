//! `module_*` MCP tools —module CRUD inside a project.
//!
//! 5 tools:
//! - `module_list`
//! - `module_create`
//! - `module_update`
//! - `module_delete`
//! - `module_move`
//!
//! The tool names and their argument names are unchanged from the
//! three-table layout: to an LLM client `module_id` still means "the id
//! of a module", and renaming the tools would break every configured
//! client for no semantic gain. Only the storage they read and write
//! changed —a module is a `nodes` row with `type = 'module'` now, and
//! `project_id` is its `root_id`.
//!
//! `module_list` reuses `super::ProjectIdArgs` (a module is owned by a
//! project). The other Args structs are private to this module.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{ResourceNode, ResourceNodePatch, ResourceNodeType};
use simple_post_core::error::AppError;

/// Resolve the project a module belongs to, for the per-project permission
/// gate. A module is a `nodes` row, so its project is the denormalised
/// `root_id` —one indexed read, no parent walk.
fn owning_project(pool: &simple_post_core::db::DbPool, node_id: &str) -> Result<String, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
}

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
        simple_post_core::db::nodes::list_by_root_and_type(
            c,
            &a.project_id,
            ResourceNodeType::Module,
        )
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
    let now = simple_post_core::domain::now_ms();
    let m = ResourceNode {
        id: format!("mod_{now}"),
        root_id: a.project_id.clone(),
        // A module's parent is the project, not another node.
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: a.name,
        description: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let id = m.id.clone();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::create(c, &m, None)
    })?;
    let stored =
        simple_post_core::db::with_conn(&state.pool, |c| simple_post_core::db::nodes::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_module_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ModuleUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_update args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.module_id)?,
        "module_update",
    )
    .await?;
    let patch = ResourceNodePatch {
        name: a.name.clone(),
        // `module_update` must never re-parent: that is `module_move`'s
        // job, and it is gated separately. `Some(None)` would promote the
        // module to the top of its project, so the absent-key form is the
        // correct "leave it alone".
        parent_id: None,
        description: None,
        sort_order: None,
    };
    let m = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::update(c, &a.module_id, patch.clone())
    })?;
    Ok(CallToolResult::json(m))
}

pub(super) async fn tool_module_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let module_id = arg_str(&args, "module_id")?.to_string();
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &module_id)?,
        "module_delete",
    )
    .await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::delete(c, &module_id)
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

    // A cross-project move is a real transfer, not delete-then-recreate.
    //
    // That older implementation was a data loss bug: the old schema's
    // `ON DELETE CASCADE` chain from module to collection to endpoint
    // meant deleting the module destroyed every collection under it and
    // every endpoint under those. The `nodes` layout makes the mistake
    // unrepresentable —there is no cascade to fall into and no second
    // table to half-copy.
    let module = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.module_id)
    })?;

    if module.root_id == a.new_project_id && a.new_sort_order.is_none() {
        return Ok(CallToolResult::json(json!({
            "module_id": a.module_id,
            "new_project_id": a.new_project_id,
            "moved": false,
        })));
    }

    let moved = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::move_to_root(
            c,
            &a.module_id,
            &a.new_project_id,
            a.new_sort_order.map(|v| v as i32),
        )
    })?;

    Ok(CallToolResult::json(json!({
        "module_id": moved.id,
        "new_project_id": moved.root_id,
        "moved": true,
    })))
}
