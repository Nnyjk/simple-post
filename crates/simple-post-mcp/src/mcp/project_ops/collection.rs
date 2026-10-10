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
use simple_post_core::domain::{ResourceNode, ResourceNodePatch, ResourceNodeType};
use simple_post_core::error::AppError;

/// Resolve the project a node belongs to, for the per-project permission
/// gate.
///
/// Handlers used to pass the literal `"global"` here, which is not a
/// project id: `check_permission` then compared `ReadCall` (the default
/// for a project with no row) against `Full` (what a mutating tool
/// requires) and denied every call. The nodes layout makes the correct
/// answer one indexed read — `root_id` is denormalised onto every row
/// precisely so this does not have to walk up the parent chain.
fn owning_project(pool: &simple_post_core::db::DbPool, node_id: &str) -> Result<String, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
}

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
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.module_id)?,
        "collection_list",
    )
    .await?;
    // The subtree of a module, because a nested collection's parent is
    // another collection — "collections under this module" is not a
    // one-hop parent match, and `list_by_module` only got away with it
    // by walking the same chain in SQL.
    let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&a.module_id))
    })?;
    let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
    // One indexed read of the project's collections, narrowed to the
    // subtree. Going through `root_id` rather than a table scan keeps
    // this proportional to the project, not the database.
    let project = owning_project(&state.pool, &a.module_id)?;
    let cols = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::list_by_root_and_type(
            c,
            &project,
            ResourceNodeType::Collection,
        )
    })?;
    Ok(CallToolResult::json(
        cols.into_iter()
            .filter(|n| in_subtree.contains(&n.id))
            .collect::<Vec<_>>(),
    ))
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
    // A collection's parent is a module (top level) or another collection
    // (nested). `new_module_id` and `parent_collection_id` are two names
    // for that one relationship, so pick whichever the caller supplied and
    // let `root_id` be derived from whichever node won.
    let parent_id = a
        .parent_collection_id
        .clone()
        .or_else(|| Some(a.module_id.clone()));
    // Gate against the project that will own the new collection, which is
    // the parent node's project.
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, parent_id.as_deref().unwrap_or_default())?,
        "collection_create",
    )
    .await?;
    let now = simple_post_core::domain::now_ms();
    let mut c = ResourceNode {
        id: format!("col_{now}"),
        root_id: String::new(),
        parent_id,
        kind: ResourceNodeType::Collection,
        name: a.name,
        description: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let root_id = owning_project(&state.pool, c.parent_id.as_deref().unwrap_or_default())?;
    c.root_id = root_id;
    let id = c.id.clone();
    simple_post_core::db::with_tx(&state.pool, |c2| {
        simple_post_core::db::nodes::create(c2, &c, None)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c2| {
        simple_post_core::db::nodes::get(c2, &id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_collection_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_update args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.collection_id)?,
        "collection_update",
    )
    .await?;
    let patch = ResourceNodePatch {
        parent_id: a
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
    let c = simple_post_core::db::with_tx(&state.pool, |c2| {
        simple_post_core::db::nodes::update(c2, &a.collection_id, patch.clone())
    })?;
    Ok(CallToolResult::json(c))
}

pub(super) async fn tool_collection_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str(&args, "collection_id")?.to_string();
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &collection_id)?,
        "collection_delete",
    )
    .await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::delete(c, &collection_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": collection_id })))
}

pub(super) async fn tool_collection_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_move args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.collection_id)?,
        "collection_move",
    )
    .await?;
    // Same two names, one relationship: nested wins when both are given,
    // matching the old `module_id` + `parent_collection_id` pair where
    // the caller had to supply a module even for a nested move.
    let new_parent = a
        .new_parent_collection_id
        .clone()
        .or_else(|| Some(a.new_module_id.clone()))
        .unwrap_or_default();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::move_many(
            c,
            std::slice::from_ref(&a.collection_id),
            Some(&new_parent),
            a.new_sort_order.unwrap_or(0) as i32,
        )
    })?;
    Ok(CallToolResult::json(json!({
        "collection_id": a.collection_id,
        "new_module_id": a.new_module_id,
        "new_parent_collection_id": a.new_parent_collection_id,
    })))
}
