//! Environment-domain MCP tools — 8 tools total.
//!
//! Every tool in this file:
//!   1. Parses its `arguments` (small typed structs via serde).
//!   2. Calls `crate::permissions::require`.
//!   3. Delegates to `crate::db::environments::*`.
//!   4. Wraps the result in a `CallToolResult::json` so it goes back to
//!      the LLM as a single text-content block.
//!
//! ## Tools in this file
//!
//! | tool                | verb      | mutating?       |
//! |---------------------|-----------|----------------|
//! | env_list            | list      | no             |
//! | env_get_active      | list      | no             |
//! | env_create          | create    | yes            |
//! | env_update          | update    | yes            |
//! | env_delete          | delete    | yes            |
//! | env_set_var         | update    | yes            |
//! | env_set_baseurl     | update    | yes            |
//! | env_set_active      | update    | yes            |

use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::{Environment, EnvironmentPatch};
use crate::error::AppError;
use crate::mcp::{
    arg_str, AppState, CallToolResult, ToolEntry,
};
use crate::tool_fn;

// =========================================================================
// Args shapes (private)
// =========================================================================

#[derive(Debug, Deserialize)]
struct ProjectIdArgs {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateEnvArgs {
    project_id: String,
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateEnvArgs {
    env_id: String,
    #[serde(default)]
    patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct EnvIdArgs {
    env_id: String,
}

#[derive(Debug, Deserialize)]
struct SetVarArgs {
    env_id: String,
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct SetBaseUrlArgs {
    env_id: String,
    definition_id: String,
    url: String,
}

// =========================================================================
// Public: dispatch table fragment
// =========================================================================

/// Return the 8 env_* rows. Called once by `mcp::mod::tool_table`.
pub fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "env_list",
            description: "List all environments in a project.",
            schema: json!({
                "type": "object",
                "properties": { "project_id": { "type": "string" } },
                "required": ["project_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_list),
        },
        ToolEntry {
            name: "env_get_active",
            description: "Return the project's currently-active environment, if any.",
            schema: json!({
                "type": "object",
                "properties": { "project_id": { "type": "string" } },
                "required": ["project_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_get_active),
        },
        ToolEntry {
            name: "env_create",
            description: "Create a new environment in a project. Returns the new Environment.",
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
            func: tool_fn!(tool_env_create),
        },
        ToolEntry {
            name: "env_update",
            description: "Update an environment's name/baseUrls/variables.",
            schema: json!({
                "type": "object",
                "properties": {
                    "env_id": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "baseUrls": { "type": "object" },
                            "variables": { "type": "object" }
                        },
                        "additionalProperties": true
                    }
                },
                "required": ["env_id", "patch"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_update),
        },
        ToolEntry {
            name: "env_delete",
            description: "Delete an environment by id.",
            schema: json!({
                "type": "object",
                "properties": { "env_id": { "type": "string" } },
                "required": ["env_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_delete),
        },
        ToolEntry {
            name: "env_set_var",
            description: "Set (or replace) a single variable in an environment.",
            schema: json!({
                "type": "object",
                "properties": {
                    "env_id": { "type": "string" },
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["env_id", "key", "value"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_set_var),
        },
        ToolEntry {
            name: "env_set_baseurl",
            description: "Set the URL for a (env, baseUrl definition) pair.",
            schema: json!({
                "type": "object",
                "properties": {
                    "env_id": { "type": "string" },
                    "definition_id": { "type": "string" },
                    "url": { "type": "string" }
                },
                "required": ["env_id", "definition_id", "url"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_set_baseurl),
        },
        ToolEntry {
            name: "env_set_active",
            description: "Mark an environment as the active one for its project.",
            schema: json!({
                "type": "object",
                "properties": { "env_id": { "type": "string" } },
                "required": ["env_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_set_active),
        },
    ]
}

// =========================================================================
// Tool impls
// =========================================================================

async fn tool_env_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_list").await?;
    let envs =
        crate::db::with_conn(&state.pool, |c| crate::db::environments::list_by_project(c, &a.project_id))?;
    Ok(CallToolResult::json(envs))
}

async fn tool_env_get_active(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_get_active args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_get_active").await?;
    let env = crate::db::with_conn(&state.pool, |c| {
        crate::db::environments::get_active(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(env))
}

async fn tool_env_create(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: CreateEnvArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_create").await?;

    let env = Environment {
        id: crate::domain::now_ms().to_string(),
        project_id: a.project_id.clone(),
        name: a.name,
        base_urls: std::collections::HashMap::new(),
        variables: std::collections::HashMap::new(),
        is_active: false,
    };
    let id = env.id.clone();
    crate::db::with_tx(&state.pool, |c| crate::db::environments::create(c, &env))?;
    let stored = crate::db::with_conn(&state.pool, |c| crate::db::environments::get(c, &id))?;
    Ok(CallToolResult::json(stored))
}

async fn tool_env_update(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: UpdateEnvArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_update args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "env_update").await?;

    // Pull each optional field out of the patch.
    let patch = EnvironmentPatch {
        name: a
            .patch
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(String::from),
        base_urls: a
            .patch
            .as_ref()
            .and_then(|p| p.get("baseUrls"))
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        variables: a
            .patch
            .as_ref()
            .and_then(|p| p.get("variables"))
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        is_active: a
            .patch
            .as_ref()
            .and_then(|p| p.get("is_active"))
            .and_then(Value::as_bool),
    };

    let env = crate::db::with_conn(&state.pool, |c| {
        crate::db::environments::update(c, &a.env_id, patch)
    })?;
    Ok(CallToolResult::json(env))
}

async fn tool_env_delete(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let env_id = arg_str(&args, "env_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "env_delete").await?;
    crate::db::with_tx(&state.pool, |c| crate::db::environments::delete(c, &env_id))?;
    Ok(CallToolResult::json(json!({ "deleted": env_id })))
}

async fn tool_env_set_var(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: SetVarArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_set_var args: {e}")))?;
    crate::mcp::check_permission(&state.pool, "global", "env_update").await?;

    // env_set_var is implemented as "read, merge into variables, write".
    let mut env = crate::db::with_conn(&state.pool, |c| crate::db::environments::get(c, &a.env_id))?;
    env.variables.insert(a.key.clone(), a.value.clone());
    let patch = EnvironmentPatch {
        variables: Some(env.variables.clone()),
        ..Default::default()
    };
    crate::db::with_conn(&state.pool, |c| crate::db::environments::update(c, &a.env_id, patch))?;

    Ok(CallToolResult::json(json!({
        "env_id": a.env_id,
        "key": a.key,
        "value": a.value,
    })))
}

async fn tool_env_set_baseurl(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: SetBaseUrlArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_set_baseurl args: {e}")))?;
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }
    crate::mcp::check_permission(&state.pool, "global", "env_update").await?;

    let mut env = crate::db::with_conn(&state.pool, |c| crate::db::environments::get(c, &a.env_id))?;
    env.base_urls.insert(a.definition_id.clone(), a.url.clone());
    let patch = EnvironmentPatch {
        base_urls: Some(env.base_urls.clone()),
        ..Default::default()
    };
    crate::db::with_conn(&state.pool, |c| crate::db::environments::update(c, &a.env_id, patch))?;

    Ok(CallToolResult::json(json!({
        "env_id": a.env_id,
        "definition_id": a.definition_id,
        "url": a.url,
    })))
}

async fn tool_env_set_active(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let env_id = arg_str(&args, "env_id")?.to_string();
    crate::mcp::check_permission(&state.pool, "global", "env_update").await?;
    let env = crate::db::with_tx(&state.pool, |c| crate::db::environments::set_active(c, &env_id))?;
    Ok(CallToolResult::json(env))
}