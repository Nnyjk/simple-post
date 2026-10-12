//! `base_url_def_*` MCP tools — services owned by a **project**.
//!
//! 5 tools:
//! - `base_url_def_list`
//! - `base_url_def_create`
//! - `base_url_def_update`
//! - `base_url_def_delete`
//! - `base_url_def_set_default`
//!
//! Every one of them is project-scoped. 「服务应该绑定到项目，这样每一个模块都会
//! 全部包含这些服务」 — a module owns no services, so an endpoint of any module
//! can resolve any of them. The module still matters, but only as a key
//! into the environment's value layer
//! (`moduleBaseUrls[defId][moduleId]`), and that lookup is not a tool.
//!
//! `base_url_def_set_default` writes the project's ★
//! (`Project.defaultBaseUrlDefinitionId`), the pointer an endpoint naming
//! no service of its own resolves against.
//!
//! The ids are distinct from the Tauri / REST commands, which use the
//! plural `base_url_defs_*`.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::BaseUrlDefinition;
use simple_post_core::error::AppError;

/// Resolve the project that owns a service, for the per-project
/// permission gate.
///
/// `def → project` rather than a literal `"global"`: the agent
/// permission table's `project_id` is a foreign key, so a bogus key finds
/// no row, falls back to the default `ReadCall`, and every write tool —
/// which requires `Full` — is denied. That failure looks like a
/// correctly-registered tool that never works.
fn owning_project(
    pool: &simple_post_core::db::DbPool,
    definition_id: &str,
) -> Result<String, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::base_url_defs::project_id_of(c, definition_id)
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct BaseUrlDefCreateArgs {
    /// The project the service belongs to. Every module of it can see
    /// the service.
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
    /// The project whose ★ is being set.
    pub project_id: String,
    /// The service to mark. Must belong to that same project.
    ///
    /// Optional, and `null` means "this project has no ★ at all" — which
    /// is the state a freshly created project is in, and a real one to be
    /// able to return to. Endpoints naming no service then resolve to an
    /// empty base URL, full stop. The GUI (`base_url_defs_set_default`
    /// with `null`) and REST (`POST /api/projects/:projectId/baseurls/
    /// default` with `"defId": null`) can both express it, so leaving it
    /// out here would make the MCP surface the only one of the three
    /// that cannot.
    #[serde(default)]
    pub definition_id: Option<String>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "base_url_def_list",
        description: "List a project's baseUrl services, flat. Every module of the project can resolve every one of them.",
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
        description: "Create a baseUrl service in a project. Services belong to the project, not to a module, so every module of it can resolve the service; the URL per (environment x module) is set separately.",
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
        description: "Rename a baseUrl service, or change its description. The service and the values pointing at it are untouched.",
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
        description: "Delete a baseUrl service. The project's ★ pointer is cleared with it if it named this service.",
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
        description: "Mark one of a project's services as the project ★ (the one its endpoints resolve against when they name no service). Pass definition_id: null to leave the project with no ★ — those endpoints then resolve to an empty base URL.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "definition_id": {
                    "type": ["string", "null"],
                    "description": "Service id within this project, or null to clear the project's ★."
                }
            },
            "required": ["project_id"],
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
    let defs = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::base_url_defs::list_by_project(c, &a.project_id)
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
    // The service is created into the project the caller named, so that
    // project's permission row is the one that governs it.
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_create").await?;

    let def = BaseUrlDefinition {
        id: format!("def_{}", simple_post_core::domain::now_ms()),
        name: a.name,
        description: a.description,
    };
    let stored = def.clone();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::base_url_defs::create(c, &def, &a.project_id)
    })?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_base_url_def_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_update args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.definition_id)?,
        "base_url_def_update",
    )
    .await?;
    let desc = a.description.as_ref().map(|d| d.as_ref().map(String::from));
    let d = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::base_url_defs::update(c, &a.definition_id, a.name.clone(), desc)
    })?;
    Ok(CallToolResult::json(d))
}

pub(super) async fn tool_base_url_def_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_delete args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.definition_id)?,
        "base_url_def_delete",
    )
    .await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::base_url_defs::delete(c, &a.definition_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.definition_id })))
}

pub(super) async fn tool_base_url_def_set_default(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: BaseUrlDefSetDefaultArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("base_url_def_set_default args: {e}")))?;
    // The project is the scope, so it is what the gate keys on. The `?`
    // matters: dropping it here is how a write tool ends up looking
    // registered but permanently denied.
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_set_default").await?;
    // `set_default` takes `Option<&str>` and rejects a service belonging
    // to another project, so `None` clears the ★ without bypassing that
    // check. There is one ★ per project, so there is nothing to promote
    // it to when the service it named is deleted.
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::base_url_defs::set_default(
            c,
            &a.project_id,
            a.definition_id.as_deref(),
        )
    })?;
    Ok(CallToolResult::json(json!({
        "project_id": a.project_id,
        "default_definition_id": a.definition_id,
    })))
}
