//! Project / module / collection / endpoint / baseUrl-definition MCP tools.
//!
//! Total: 5 + 5 + 5 + 8 + 5 = **28 tools**.
//!
//! ## Permission naming
//!
//! [`crate::permissions::required_permission`] maps tool names
//! to a `Permission` rank (`Read` < `ReadCall` < `Full`). The map is
//! keyed by the *tool* name, so call sites use e.g. `endpoint_get` and
//! `endpoint_execute` (the latter is `api_call_endpoint` here, since it
//! actually issues a network call).
//!
//! ## Permission gating strategy
//!
//! For tools that take `project_id` directly we pass it to
//! [`crate::permissions::require`]. For `endpoint_*` tools that don't
//! (get / delete / duplicate / move / search), we look up the endpoint
//! first to find its `project_id`, then gate.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{
    BaseUrlDefinition, Collection, CollectionPatch, Endpoint, EndpointPatch,
    Module, ModulePatch, Project, ProjectPatch, RequestBody,
};
use crate::error::AppError;
use crate::mcp::{
    arg_str, arg_str_opt, AppState, CallToolResult, ToolEntry,
};
use crate::tool_fn;

// =========================================================================
// Args shapes
// =========================================================================

#[derive(Debug, Deserialize)]
struct ProjectIdArgs {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct ModuleIdArgs {
    module_id: String,
}

#[derive(Debug, Deserialize)]
struct CollectionCreateArgs {
    module_id: String,
    #[serde(default)]
    parent_collection_id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct EndpointCreateArgs {
    collection_id: String,
    name: String,
    method: String,
    url: String,
    #[serde(default)]
    headers: Option<Vec<crate::domain::KeyValue>>,
    #[serde(default)]
    params: Option<Vec<crate::domain::KeyValue>>,
    #[serde(default)]
    body_mode: Option<String>,
    #[serde(default)]
    body_content: Option<String>,
    #[serde(default)]
    auth_type: Option<String>,
    #[serde(default)]
    auth_config: Option<Value>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct EndpointMoveArgs {
    endpoint_id: String,
    new_collection_id: String,
    #[serde(default)]
    new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CollectionMoveArgs {
    collection_id: String,
    new_module_id: String,
    #[serde(default)]
    new_parent_collection_id: Option<String>,
    #[serde(default)]
    new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ModuleMoveArgs {
    module_id: String,
    new_project_id: String,
    #[serde(default)]
    new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ProjectCreateArgs {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectUpdateArgs {
    project_id: String,
    #[serde(default)]
    patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ProjectDeleteArgs {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct EndpointSearchArgs {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    module_id: Option<String>,
    #[serde(default)]
    collection_id: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct EndpointGetArgs {
    endpoint_id: String,
}

#[derive(Debug, Deserialize)]
struct EndpointDeleteArgs {
    endpoint_id: String,
}

#[derive(Debug, Deserialize)]
struct EndpointDuplicateArgs {
    endpoint_id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleCreateArgs {
    project_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ModuleUpdateArgs {
    module_id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CollectionUpdateArgs {
    collection_id: String,
    #[serde(default)]
    patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct BaseUrlDefCreateArgs {
    project_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaseUrlDefUpdateArgs {
    definition_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct BaseUrlDefDeleteArgs {
    definition_id: String,
}

#[derive(Debug, Deserialize)]
struct BaseUrlDefSetDefaultArgs {
    project_id: String,
    definition_id: String,
}

// =========================================================================
// Dispatch table
// =========================================================================

pub fn table() -> Vec<ToolEntry> {
    let mut v = Vec::with_capacity(32);

    // ---- projects (5) ----
    v.push(ToolEntry {
        name: "project_list",
        description: "List all projects.",
        schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        func: tool_fn!(tool_project_list),
    });
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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

    // ---- modules (5) ----
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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

    // ---- collections (5) ----
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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

    // ---- endpoints (8) ----
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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

    // ---- base_url_defs (5) ----
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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
    v.push(ToolEntry {
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

    v
}

// =========================================================================
// Project tool impls
// =========================================================================

async fn tool_project_list(state: &AppState, _args: Value) -> Result<CallToolResult, AppError> {
    crate::mcp::check_permission(&state.pool, "global", "project_list").await?;
    let projects = crate::db::with_conn(&state.pool, |c| crate::db::projects::list(c))?;
    Ok(CallToolResult::json(projects))
}

async fn tool_project_get(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_get args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_get").await?;
    let p = crate::db::with_conn(&state.pool, |c| crate::db::projects::get(c, &a.project_id))?;
    Ok(CallToolResult::json(p))
}

async fn tool_project_create(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, "global", "project_create").await?;
    let now = crate::domain::now_ms();
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
    crate::db::with_tx(&state.pool, |c| crate::db::projects::create(c, &p))?;
    let stored = crate::db::with_conn(&state.pool, |c| crate::db::projects::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

async fn tool_project_update(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_update").await?;

    let patch = ProjectPatch {
        name: a.patch.as_ref().and_then(|p| p.get("name")).and_then(Value::as_str).map(String::from),
        description: a
            .patch
            .as_ref()
            .and_then(|p| p.get("description"))
            .map(|v| v.as_str().map(String::from)),
        base_url: None,
        base_url_definitions: None,
        default_base_url_definition_id: None,
        color: a.patch.as_ref().and_then(|p| p.get("color")).and_then(Value::as_str).map(String::from),
    };
    let p = crate::db::with_conn(&state.pool, |c| {
        crate::db::projects::update(c, &a.project_id, patch)
    })?;
    Ok(CallToolResult::json(p))
}

async fn tool_project_delete(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("project_delete args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "project_delete").await?;
    crate::db::with_tx(&state.pool, |c| crate::db::projects::delete(c, &a.project_id))?;
    Ok(CallToolResult::json(json!({ "deleted": a.project_id })))
}

// =========================================================================
// Module tool impls
// =========================================================================

async fn tool_module_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "module_list").await?;
    let mods = crate::db::with_conn(&state.pool, |c| {
        crate::db::modules::list_by_project(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(mods))
}

async fn tool_module_create(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ModuleCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, &a.project_id, "module_create").await?;
    let m = Module {
        id: format!("mod_{}", crate::domain::now_ms()),
        project_id: a.project_id.clone(),
        name: a.name,
        description: None,
        sort_order: 0,
        expanded: None,
    };
    let id = m.id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::modules::create(c, &m))?;
    let stored = crate::db::with_conn(&state.pool, |c| crate::db::modules::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

async fn tool_module_update(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ModuleUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "module_update").await?;
    let patch = ModulePatch {
        name: a.name.clone(),
        description: None,
        sort_order: None,
    };
    let m = crate::db::with_conn(&state.pool, |c| {
        crate::db::modules::update(c, &a.module_id, patch)
    })?;
    Ok(CallToolResult::json(m))
}

async fn tool_module_delete(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let module_id = arg_str(&args, "module_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "module_delete").await?;
    crate::db::with_tx(&state.pool, |c| crate::db::modules::delete(c, &module_id))?;
    Ok(CallToolResult::json(json!({ "deleted": module_id })))
}

async fn tool_module_move(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ModuleMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("module_move args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.new_project_id, "module_move").await?;

    // `modules::update` doesn't expose a `project_id` field on
    // ModulePatch — moving a module between projects isn't a first-class
    // operation. Implement as delete-then-create to keep the FK happy.
    let module = crate::db::with_conn(&state.pool, |c| crate::db::modules::get(c, &a.module_id))?;
    crate::db::with_tx(&state.pool, |c| crate::db::modules::delete(c, &a.module_id))?;
    let new_id = format!("mod_{}", crate::domain::now_ms());
    let new_mod = Module {
        id: new_id.clone(),
        project_id: a.new_project_id.clone(),
        name: module.name,
        description: module.description,
        sort_order: a.new_sort_order.unwrap_or(0),
        expanded: None,
    };
    crate::db::with_tx(&state.pool, |c| crate::db::modules::create(c, &new_mod))?;
    Ok(CallToolResult::json(json!({
        "module_id": new_id,
        "new_project_id": a.new_project_id,
    })))
}

// =========================================================================
// Collection tool impls
// =========================================================================

async fn tool_collection_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ModuleIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "collection_list").await?;
    let cols = crate::db::with_conn(&state.pool, |c| {
        crate::db::collections::list_by_module(c, &a.module_id)
    })?;
    Ok(CallToolResult::json(cols))
}

async fn tool_collection_create(
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
        id: format!("col_{}", crate::domain::now_ms()),
        module_id: a.module_id,
        parent_collection_id: a.parent_collection_id,
        name: a.name,
        description: None,
        sort_order: 0,
        expanded: None,
    };
    let id = c.id.clone();
    crate::db::with_tx(&state.pool, |c2| crate::db::collections::create(c2, &c))?;
    let stored = crate::db::with_conn(&state.pool, |c2| crate::db::collections::get(c2, &id))?;
    Ok(CallToolResult::json(stored))
}

async fn tool_collection_update(
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
        name: a.patch.as_ref().and_then(|p| p.get("name")).and_then(Value::as_str).map(String::from),
        description: a
            .patch
            .as_ref()
            .and_then(|p| p.get("description"))
            .map(|v| v.as_str().map(String::from)),
        sort_order: None,
    };
    let c = crate::db::with_conn(&state.pool, |c2| {
        crate::db::collections::update(c2, &a.collection_id, patch)
    })?;
    Ok(CallToolResult::json(c))
}

async fn tool_collection_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str(&args, "collection_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "collection_delete").await?;
    crate::db::with_tx(&state.pool, |c| crate::db::collections::delete(c, &collection_id))?;
    Ok(CallToolResult::json(json!({ "deleted": collection_id })))
}

async fn tool_collection_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_move args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "collection_move").await?;
    let patch = CollectionPatch {
        module_id: Some(a.new_module_id.clone()),
        parent_collection_id: a.new_parent_collection_id.map(|s| Some(s)),
        ..Default::default()
    };
    crate::db::with_conn(&state.pool, |c| {
        crate::db::collections::update(c, &a.collection_id, patch)
    })?;
    Ok(CallToolResult::json(json!({
        "collection_id": a.collection_id,
        "new_module_id": a.new_module_id,
    })))
}

// =========================================================================
// Endpoint tool impls
// =========================================================================

async fn tool_endpoint_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
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

async fn tool_endpoint_get(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: EndpointGetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_get args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &ep.collection_id);
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_get").await?;
    Ok(CallToolResult::json(ep))
}

async fn tool_endpoint_search(
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

async fn tool_endpoint_create(
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

async fn tool_endpoint_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let endpoint_id = arg_str(&args, "endpoint_id")?.to_string();
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &ep.collection_id);
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

async fn tool_endpoint_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_delete args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &ep.collection_id);
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_delete").await?;
    crate::db::with_tx(&state.pool, |c| {
        crate::db::endpoints::delete(c, &a.endpoint_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.endpoint_id })))
}

async fn tool_endpoint_duplicate(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDuplicateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_duplicate args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &ep.collection_id);
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

async fn tool_endpoint_move(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: EndpointMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_move args: {e}")))?;
    let ep = crate::db::with_conn(&state.pool, |c| {
        crate::db::endpoints::get(c, &a.endpoint_id)
    })?;
    let project_id = endpoint_project_id(&state.pool, &ep.collection_id);
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
// BaseUrlDefinition tool impls
// =========================================================================

async fn tool_base_url_def_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_list").await?;
    let defs = crate::db::with_conn(&state.pool, |c| {
        crate::db::base_url_defs::list_by_project(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(defs))
}

async fn tool_base_url_def_create(
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

async fn tool_base_url_def_update(
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

async fn tool_base_url_def_delete(
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

async fn tool_base_url_def_set_default(
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

// =========================================================================
// Helpers
// =========================================================================

/// Resolve the project_id for an endpoint via `collection → module → project`.
/// `Endpoint` itself doesn't denormalise project_id, so any tool that
/// wants to permission-gate an endpoint must walk the chain. Returns the
/// empty string if any step fails — callers should treat that as a
/// "no project" sentinel the permission layer recognises (matches the
/// behaviour in `mcp/http_ops.rs::endpoint_collection_project_id`).
fn endpoint_project_id(pool: &crate::db::DbPool, collection_id: &str) -> String {
    if let Ok(coll) = crate::db::with_conn(pool, |c| {
        crate::db::collections::get(c, collection_id)
    }) {
        if let Ok(module) = crate::db::with_conn(pool, |c| {
            crate::db::modules::get(c, &coll.module_id)
        }) {
            return module.project_id;
        }
    }
    String::new()
}

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
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    /// Per-call counter that disambiguates temp dirs when several tests
    /// construct their state() in the same millisecond (cargo's default
    /// runs tests in parallel).
    static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn state() -> Arc<AppState> {
        let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "simple-post-mcp-project-ops-test-{}-{}-{}",
            std::process::id(),
            crate::domain::now_ms(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(crate::mcp::AppState {
            pool: crate::db::init(&dir).unwrap(),
            started_at: Instant::now(),
            metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
        })
    }

    #[test]
    fn table_count_is_28() {
        assert_eq!(table().len(), 28);
    }

    #[tokio::test]
    async fn project_create_rejects_empty_name() {
        let res = tool_project_create(&state(), json!({ "name": "  " })).await;
        assert!(matches!(res, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn endpoint_create_rejects_empty_url() {
        let res = tool_endpoint_create(
            &state(),
            json!({
                "collection_id": "c1",
                "name": "n",
                "method": "GET",
                "url": ""
            }),
        )
        .await;
        assert!(matches!(res, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn endpoint_create_rejects_unknown_method_gracefully() {
        // Empty method is malformed HTTP — surface a Validation error
        // (the LLM / UI gets `is_error=true` text) instead of panicking
        // or persisting garbage. Custom token-shape verbs like
        // "PROPFIND" / "FROBNICATE" still pass through as
        // `HttpMethod::Custom`.
        let res = tool_endpoint_create(
            &state(),
            json!({
                "collection_id": "c1",
                "name": "n",
                "method": "",
                "url": "/x"
            }),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "expected Validation rejection, got {res:?}"
        );
    }

    #[test]
    fn parse_body_mode_known_values() {
        assert!(parse_body_mode("json").is_ok());
        assert!(parse_body_mode("form").is_ok());
        assert!(parse_body_mode("raw").is_ok());
        assert!(parse_body_mode("none").is_ok());
        assert!(parse_body_mode("wat").is_err());
    }

    #[test]
    fn build_auth_handles_each_kind() {
        let none = build_auth(Some("none"), None).unwrap();
        assert_eq!(none.kind, crate::domain::AuthType::None);

        let bearer = build_auth(Some("bearer"), Some(json!({"bearer": "tok"}))).unwrap();
        assert_eq!(bearer.kind, crate::domain::AuthType::Bearer);

        let unknown = build_auth(Some("magic"), None);
        assert!(unknown.is_err());
    }

    // HashMap import smoke
    #[allow(dead_code)]
    fn _hm() -> HashMap<String, String> {
        HashMap::new()
    }
}