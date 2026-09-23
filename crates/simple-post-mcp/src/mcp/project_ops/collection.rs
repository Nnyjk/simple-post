//! `collection_*` MCP tools — collection CRUD and movement within a module.
//!
//! 5 tools:
//! - `collection_list`
//! - `collection_create`
//! - `collection_update`
//! - `collection_delete`
//! - `collection_move`
//!
//! `collection_list` reuses `super::ModuleIdArgs`. All other Args structs
//! are local to this module.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{Collection, CollectionPatch};
use simple_post_core::error::AppError;

#[derive(Debug, Deserialize)]
pub(super) struct CollectionCreateArgs {
    pub module_id: String,
    #[serde(default)]
    pub parent_collection_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CollectionUpdateArgs {
    pub collection_id: String,
    #[serde(default)]
    pub patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CollectionMoveArgs {
    pub collection_id: String,
    pub new_module_id: String,
    #[serde(default)]
    pub new_parent_collection_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // reserved for future sort-order API
    pub new_sort_order: Option<i64>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "collection_list",
        description: "List collections in a module (top-level + nested).",
        schema: json!({
            "type": "object",
            "properties": { "module_id": { "type": "string" } },
            "required": ["module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_create",
        description: "Create a collection (top-level if parent_collection_id is omitted).",
        schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string" },
                "parent_collection_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["module_id", "name"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_update",
        description: "Patch a collection's name/description/parent.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "patch": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "parentCollectionId": { "type": ["string", "null"] }
                    },
                    "additionalProperties": true
                }
            },
            "required": ["collection_id", "patch"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_delete",
        description: "Delete a collection (and its nested collections + endpoints).",
        schema: json!({
            "type": "object",
            "properties": { "collection_id": { "type": "string" } },
            "required": ["collection_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_move",
        description: "Move a collection to a new module / parent / position.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "new_module_id": { "type": "string" },
                "new_parent_collection_id": { "type": "string" },
                "new_sort_order": { "type": "integer" }
            },
            "required": ["collection_id", "new_module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_move),
    });
}

pub(super) async fn tool_collection_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: super::ModuleIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "collection_list").await?;
    let cols = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::collections::list_by_module(c, &a.module_id)
    })?;
    Ok(CallToolResult::json(cols))
}

pub(super) async fn tool_collection_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, "global", "collection_create").await?;
    let c = Collection {
        id: format!("col_{}", simple_post_core::domain::now_ms()),
        module_id: a.module_id,
        parent_collection_id: a.parent_collection_id,
        name: a.name,
        description: None,
        sort_order: 0,
        expanded: None,
    };
    let id = c.id.clone();
    simple_post_core::db::with_tx(&state.pool, |c2| {
        simple_post_core::db::collections::create(c2, &c)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c2| {
        simple_post_core::db::collections::get(c2, &id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_collection_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "collection_update").await?;
    let patch = CollectionPatch {
        module_id: None,
        parent_collection_id: a
            .patch
            .as_ref()
            .and_then(|p| p.get("parentCollectionId"))
            .map(|v| v.as_str().map(String::from)),
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
        sort_order: None,
    };
    let c = simple_post_core::db::with_conn(&state.pool, |c2| {
        simple_post_core::db::collections::update(c2, &a.collection_id, patch)
    })?;
    Ok(CallToolResult::json(c))
}

pub(super) async fn tool_collection_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str(&args, "collection_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "collection_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::collections::delete(c, &collection_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": collection_id })))
}

pub(super) async fn tool_collection_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_move args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "collection_move").await?;
    let patch = CollectionPatch {
        module_id: Some(a.new_module_id.clone()),
        parent_collection_id: a.new_parent_collection_id.map(Some),
        ..Default::default()
    };
    simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::collections::update(c, &a.collection_id, patch)
    })?;
    Ok(CallToolResult::json(json!({
        "collection_id": a.collection_id,
        "new_module_id": a.new_module_id,
    })))
}
