//! `module_*` MCP 工具 —— 项目内的模块 CRUD，共 5 个。
//!
//! 工具名与参数名沿用三表时期的老名字：对 LLM 客户端而言 `module_id` 仍然是
//! 「模块的 id」，改名只会白白打断所有已配置的客户端。变的只有存储 —— 模块现在是
//! `type = 'module'` 的 `nodes` 行，所属项目就是它的 `root_id`。
//!
//! `module_list` 复用 `super::ProjectIdArgs`（模块归属于项目）；其余 Args 私有。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{ResourceNodePatch, ResourceNodeType};
use simple_post_core::error::AppError;

/// 解析模块所属项目，供按项目的那扇门使用。模块是 `nodes` 行，所属项目就是反规范化
/// 在行上的 `root_id` —— 一次索引读，不必回溯父链。
fn owning_project(pool: &simple_post_core::db::DbPool, node_id: &str) -> Result<String, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct ModuleCreateArgs {
    pub project_id: String,
    pub name: String,
    /// 项目里已经有同名模块时怎么办。缺省是 `reuse`。
    #[serde(default)]
    pub if_exists: Option<String>,
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
        description: "Create a module in a project. When the project already has a \
                       module with the same name, `if_exists=reuse` (the default) returns \
                       it instead of creating a second one.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "name": { "type": "string" },
                "if_exists": {
                    "type": "string",
                    "enum": ["reuse", "error", "duplicate"],
                    "description": "What to do when the project already has a module with this name. \"reuse\" (default) returns the existing one; \"error\" refuses; \"duplicate\" creates a second one."
                }
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
    let if_exists = simple_post_core::spec::IfExists::parse(a.if_exists.as_deref().unwrap_or(""))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "module_create").await?;

    // 「按名找或建」与 id 的生成都在 `core::spec`，REST 路由与桌面 command 共用。
    let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::spec::resolve_module(c, &a.project_id, &a.name, if_exists)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &resolved.id)
    })?;
    let mut out = serde_json::to_value(&stored)
        .map_err(|e| AppError::Validation(format!("module response: {e}")))?;
    out["created"] = json!(resolved.created);
    Ok(CallToolResult::json(out))
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
        // `module_update` 绝不改父级 —— 那是 `module_move` 的职责，且单独设门。
        // `Some(None)` 会把模块提到项目顶层，所以这里必须用「键缺失」表示「别动」。
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

    // 跨项目移动是改写归属，不是「删掉再建一个」：`move_to_root` 重写整棵子树的
    // `root_id`，而删除会把模块下面的集合与端点一起带走。
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
