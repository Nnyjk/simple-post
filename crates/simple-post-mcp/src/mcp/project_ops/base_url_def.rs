//! `base_url_def_*` MCP 工具 —— 服务由**项目**持有，共 5 个。
//!
//! 全部按项目作用域：「服务绑定到项目，每一个模块都看得到全部服务」—— 模块本身不
//! 持有服务，所以任何模块下的端点都能解析其中任何一个。模块仍然重要，但只作为取值层
//! 的键（`moduleBaseUrls[defId][moduleId]`），而那次查表并不是一个工具。
//!
//! `base_url_def_set_default` 写的是项目的 ★（`Project.defaultBaseUrlDefinitionId`），
//! 即没点名任何服务的端点据以解析的那个指针。
//!
//! 工具名与 Tauri / REST 命令不同 —— 后者用复数 `base_url_defs_*`。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::BaseUrlDefinition;
use simple_post_core::error::AppError;

/// 解析服务所属项目，供按项目的那扇门使用。
///
/// 要的是 `def → project`，而不是写死字面量 "global"：agent 权限表的 `project_id`
/// 是外键，假 id 查不到行、退回默认 `ReadCall`，于是要求 `Full` 的写工具全被拒 ——
/// 症状是「注册得好好的工具从来不能用」。
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
    /// 服务所属项目。它的每一个模块都看得到这个服务。
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
    /// 要设置 ★ 的项目。
    pub project_id: String,
    /// 要标记的服务，必须属于同一个项目。
    ///
    /// 可选，`null` 表示「这个项目没有 ★」—— 新建项目本来就是这个状态，之后也随时
    /// 可以回到它；此时没点名服务的端点一律解析出空 base URL。GUI
    /// （`base_url_defs_set_default` 传 `null`）与 REST（`POST
    /// /api/projects/:projectId/baseurls/default` 带 `"defId": null`）都能表达，
    /// 这里若不支持，MCP 就会是三者中唯一做不到的那个。
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
    // 服务建进调用方指名的那个项目，所以管它的是那个项目的权限行。
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
    // 作用域就是项目，门也按项目取键。`?` 不能丢：丢掉正是「写工具看着注册好了、
    // 却被永久拒绝」的成因。
    crate::mcp::check_permission(&state.pool, &a.project_id, "base_url_def_set_default").await?;
    // `set_default` 收 `Option<&str>` 且会拒绝跨项目的服务，所以 `None` 清除 ★ 不会
    // 绕过这项校验。每个项目只有一个 ★，它指名的服务被删时也就没有别的可顶上。
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
