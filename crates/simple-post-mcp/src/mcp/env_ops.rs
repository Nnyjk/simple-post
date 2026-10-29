//! 环境域 MCP 工具 —— 共 8 个。
//!
//! 每个工具都是同一套四步：解析 `arguments`（小型 serde 类型）→ 过
//! `simple_post_core::permissions::require` → 委托
//! `simple_post_core::db::environments::*` → 用 `CallToolResult::json` 包成单个
//! text content 块回给 LLM。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::EnvironmentPatch;
use simple_post_core::error::AppError;

// =========================================================================
// 私有 Args 形状
// =========================================================================

#[derive(Debug, Deserialize)]
struct ProjectIdArgs {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateEnvArgs {
    project_id: String,
    name: String,
    /// 已经有同名环境时怎么办。缺省是 `reuse`。
    #[serde(default)]
    if_exists: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateEnvArgs {
    env_id: String,
    #[serde(default)]
    patch: Option<Value>,
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
    /// 要写入的模块。URL 住在 (环境 × 服务 × 模块) 这一格 —— 同一个服务可以在两个
    /// 模块指向两个不同 host。
    module_id: String,
    url: String,
}

// =========================================================================
// 公共：dispatch 表片段
// =========================================================================

/// 被 `mcp::mod::tool_table` 调一次的 8 行 env_* 记录。
pub(crate) fn table() -> Vec<ToolEntry> {
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
            description: "Create an environment in a project. if_exists=reuse (the default) returns the existing environment with the same name instead of creating a second one.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "name": { "type": "string" },
                    "if_exists": {
                        "type": "string",
                        "enum": ["reuse", "error", "duplicate"],
                        "description": "What to do when the project already has an environment with this name. \"reuse\" (default) returns the existing one; \"error\" refuses; \"duplicate\" creates a second one."
                    }
                },
                "required": ["project_id", "name"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_env_create),
        },
        ToolEntry {
            name: "env_update",
            description: "Update an environment's name, moduleBaseUrls, variables and disabledVariables. Each map is replaced as a whole, not merged: send the complete set the environment should end up with. To change one variable use env_set_var.",
            schema: json!({
                "type": "object",
                "properties": {
                    "env_id": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "moduleBaseUrls": {
                                "type": "object",
                                "description": "Replaces the whole map, keyed by service id then module id. Each leaf is that (environment x service x module) cell's URL; an absent cell means no base URL, because that cell never inherits."
                            },
                            "variables": {
                                "type": "object",
                                "description": "Replaces the whole variable map. A variable that is absent here is removed."
                            },
                            "disabledVariables": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Replaces the whole set of disabled variable names. A disabled variable is still stored and shown but does not take part in substitution."
                            }
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
            description: "Set the URL one service has for one module in one environment — the (environment x service x module) cell. That cell never inherits, so an unwritten cell means the request goes out without a base URL. url must not be empty.",
            schema: json!({
                "type": "object",
                "properties": {
                    "env_id": { "type": "string" },
                    "definition_id": { "type": "string" },
                    "module_id": { "type": "string" },
                    "url": { "type": "string" }
                },
                "required": ["env_id", "definition_id", "module_id", "url"],
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
// 工具实现
// =========================================================================

async fn tool_env_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_list").await?;
    let envs = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::list_by_project(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(envs))
}

async fn tool_env_get_active(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ProjectIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_get_active args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_get_active").await?;
    let env = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::get_active(c, &a.project_id)
    })?;
    Ok(CallToolResult::json(env))
}

async fn tool_env_create(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: CreateEnvArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    let if_exists = simple_post_core::spec::IfExists::parse(a.if_exists.as_deref().unwrap_or(""))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "env_create").await?;

    // 「按名找或建」与 id 的生成都在 `core::spec`，REST 路由与桌面 command 共用。
    let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::spec::resolve_environment(c, &a.project_id, &a.name, if_exists)
    })?;
    let stored = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::get(c, &resolved.id)
    })?;
    let mut out = serde_json::to_value(&stored)
        .map_err(|e| AppError::Validation(format!("environment response: {e}")))?;
    out["created"] = json!(resolved.created);
    Ok(CallToolResult::json(out))
}

async fn tool_env_update(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: UpdateEnvArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_update args: {e}")))?;
    // 先查出所属项目，按项目那扇门才对得上键。写死字面量 "global" 会撞上默认的
    // `ReadCall` 档位，把每一次写都静默拒掉。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::lookup_project_id(c, &a.env_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "env_update").await?;

    // patch 里没点名的字段一律留 None，故 update 只写被点名的那些列。
    let patch = EnvironmentPatch {
        name: a
            .patch
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(String::from),
        // `base_urls` 固定留 None：该列已死 —— `db::migrate` 把值全搬进了
        // `moduleBaseUrls`，`db::environments::update` 也不读它，转发只会回报一次
        // 「写成功」而实际什么都没改。客户端照本工具描述走 `moduleBaseUrls`。
        base_urls: None,
        module_base_urls: a
            .patch
            .as_ref()
            .and_then(|p| p.get("moduleBaseUrls"))
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        variables: a
            .patch
            .as_ref()
            .and_then(|p| p.get("variables"))
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        disabled_variables: a
            .patch
            .as_ref()
            .and_then(|p| p.get("disabledVariables"))
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        is_active: a
            .patch
            .as_ref()
            .and_then(|p| p.get("is_active"))
            .and_then(Value::as_bool),
    };

    let env = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::update(c, &a.env_id, patch)
    })?;
    Ok(CallToolResult::json(env))
}

async fn tool_env_delete(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let env_id = arg_str(&args, "env_id")?.to_string();
    // 理由同 tool_env_update —— 过门前先查所属项目。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::lookup_project_id(c, &env_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "env_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::environments::delete(c, &env_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": env_id })))
}

async fn tool_env_set_var(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: SetVarArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("env_set_var args: {e}")))?;
    // 过门前先查所属项目，门才对得上键。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::lookup_project_id(c, &a.env_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "env_update").await?;

    // env_set_var 实现为「读 — 并入 variables — 写」。
    let mut env = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::get(c, &a.env_id)
    })?;
    env.variables.insert(a.key.clone(), a.value.clone());
    let patch = EnvironmentPatch {
        variables: Some(env.variables.clone()),
        ..Default::default()
    };
    simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::update(c, &a.env_id, patch)
    })?;

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
    // 过门前先查所属项目，门才对得上键。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::lookup_project_id(c, &a.env_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "env_update").await?;

    // 与 `env_set_var` 同为「读—合并—写」：整张 map 是一个 JSON 列，一次原子
    // UPDATE 才不会让读者看到半张 map。走 `set_module_base_url` 而不是手搓
    // patch，因为只有它知道嵌套形状；在这里 patch `baseUrls` 只会写进那列死列，
    // 真正生效的一层纹丝不动。
    let env = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::environments::set_module_base_url(
            c,
            &a.env_id,
            &a.definition_id,
            &a.module_id,
            Some(&a.url),
        )
    })?;

    Ok(CallToolResult::json(env))
}

async fn tool_env_set_active(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let env_id = arg_str(&args, "env_id")?.to_string();
    // 过门前先查所属项目，门才对得上键。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::lookup_project_id(c, &env_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "env_update").await?;
    let env = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::environments::set_active(c, &env_id)
    })?;
    Ok(CallToolResult::json(env))
}
