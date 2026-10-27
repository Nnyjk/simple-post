//! `project_*` MCP 工具 —— 项目级 CRUD，共 5 个。
//!
//! 只在本文件用到的 Args 结构放在各自工具实现旁边；共用的 `ProjectIdArgs` /
//! `ModuleIdArgs` 留在 `super`，通过 `super::ProjectIdArgs` 引入。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::ProjectPatch;
use simple_post_core::error::AppError;

#[derive(Debug, Deserialize)]
pub(super) struct ProjectCreateArgs {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// 已经有同名项目时怎么办。缺省是 `reuse`。
    #[serde(default)]
    pub if_exists: Option<String>,
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
        description: "Create a new project. When a project with the same name already \
                       exists, `if_exists=reuse` (the default) returns it instead of creating \
                       a second one.",
        schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "color": { "type": "string" },
                "if_exists": {
                    "type": "string",
                    "enum": ["reuse", "error", "duplicate"],
                    "description": "What to do when a project with this name already exists. \"reuse\" (default) returns the existing one; \"error\" refuses; \"duplicate\" creates a second one."
                }
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
    // 无按项目的门：列全部项目跨项目，没有单一归属行可查，由 Gate A（全局
    // `CapabilityPolicy`）裁决。
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
    // 无按项目的门：项目还不存在，没有归属项目、也就没有可套用的存档权限。管它的是
    // Gate A（全局 `CapabilityPolicy`，在 `mcp::dispatch` 里执行），必须 `Create` 起。
    let if_exists = simple_post_core::spec::IfExists::parse(a.if_exists.as_deref().unwrap_or(""))?;
    let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::spec::resolve_project(
            c,
            &a.name,
            a.description.as_deref(),
            a.color.as_deref(),
            if_exists,
        )
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::projects::get(c, &resolved.id)
    })?;
    let mut out = serde_json::to_value(&stored)
        .map_err(|e| AppError::Validation(format!("project response: {e}")))?;
    out["created"] = json!(resolved.created);
    Ok(CallToolResult::json(out))
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
