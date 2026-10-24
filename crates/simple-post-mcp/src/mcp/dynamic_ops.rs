//! 动态变量域 MCP 工具 —— 共 7 个。
//!
//! 每个工具都是同一套四步：解析 `arguments`（小型 serde 类型）→ 过
//! `simple_post_core::permissions::require` → 委托 `crate::dynamic::ops` → 用
//! `CallToolResult::json` 包成单个 text content 块回给 LLM。
//!
//! `dynamic_var_resolve` 会真的发出站请求，故它的 capability 是 `Send`（与
//! `api_call_endpoint` 同级），逐项目权限是 `ReadCall`。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult, ToolEntry};
use crate::tool_fn;
use simple_post_core::domain::{DynamicVariablePatch, ValueMode};
use simple_post_core::error::AppError;

// =========================================================================
// 私有 Args 形状
// =========================================================================

#[derive(Debug, Deserialize)]
struct ListArgs {
    project_id: String,
    #[serde(default)]
    environment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetArgs {
    definition_id: String,
    #[serde(default)]
    environment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateArgs {
    project_id: String,
    name: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// 数据源端点（`nodes.id`）。与 `source_inline_json` 二选一。
    #[serde(default)]
    source_endpoint_id: Option<String>,
    /// 内联请求（method / url / headers / body / auth 的 JSON 文本）。
    #[serde(default)]
    source_inline_json: Option<String>,
    /// JSONPath 表达式。`$` 表示整段响应。
    #[serde(default)]
    extract_path: Option<String>,
    /// 值的序列化形态：`auto` / `json` / `text`。
    #[serde(default)]
    value_mode: Option<ValueMode>,
    #[serde(default)]
    ttl_seconds: Option<i64>,
    /// 取数失败时是否允许用已过期的缓存值兜底。
    #[serde(default)]
    stale_fallback: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UpdateArgs {
    definition_id: String,
    patch: DynamicVariablePatch,
}

#[derive(Debug, Deserialize)]
struct DeleteArgs {
    definition_id: String,
}

#[derive(Debug, Deserialize)]
struct ResolveArgs {
    project_id: String,
    /// 要解析的变量名。请求体里写 `{{name}}` 用的那个。
    names: Vec<String>,
    #[serde(default)]
    environment_id: Option<String>,
    /// 真 = 跳过缓存，强制重新取数。
    #[serde(default)]
    force: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SetOverrideArgs {
    definition_id: String,
    /// 覆盖值。`null` 表示清除覆盖值（下次解析重新取数）。
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    environment_id: Option<String>,
    /// 覆盖值的到期时间（unix 毫秒）。缺席且 `forever` 为假时沿用定义的 TTL。
    #[serde(default)]
    expires_at: Option<i64>,
    /// 真 = 一直有效，直到手动清除。
    #[serde(default)]
    forever: Option<bool>,
}

// =========================================================================
// 公共：dispatch 表片段
// =========================================================================

/// 被 `mcp::mod::tool_table` 调一次的 7 行 dynamic_var_* 记录。
pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "dynamic_var_list",
            description: "List a project's dynamic variables with their current value and cache state. Reads the cache only; it never fetches.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "environment_id": { "type": "string" }
                },
                "required": ["project_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_list),
        },
        ToolEntry {
            name: "dynamic_var_get",
            description: "Read one dynamic variable definition with its current value and cache state.",
            schema: json!({
                "type": "object",
                "properties": {
                    "definition_id": { "type": "string" },
                    "environment_id": { "type": "string" }
                },
                "required": ["definition_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_get),
        },
        ToolEntry {
            name: "dynamic_var_create",
            description: "Create a dynamic variable in a project: a name, a data source (an endpoint id or an inline request), a JSONPath extraction rule and a cache TTL. The name must match ^[A-Za-z0-9_]+$ and is unique inside the project.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "source_endpoint_id": { "type": "string" },
                    "source_inline_json": { "type": "string" },
                    "extract_path": { "type": "string" },
                    "value_mode": { "type": "string", "enum": ["auto", "json", "text"] },
                    "ttl_seconds": { "type": "integer" },
                    "stale_fallback": { "type": "boolean" },
                    "enabled": { "type": "boolean" },
                    "sort_order": { "type": "integer" }
                },
                "required": ["project_id", "name"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_create),
        },
        ToolEntry {
            name: "dynamic_var_update",
            description: "Update a dynamic variable's definition.",
            schema: json!({
                "type": "object",
                "properties": {
                    "definition_id": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": ["string", "null"] },
                            "sourceEndpointId": { "type": ["string", "null"] },
                            "sourceInlineJson": { "type": ["string", "null"] },
                            "extractPath": { "type": "string" },
                            "valueMode": { "type": "string", "enum": ["auto", "json", "text"] },
                            "ttlSeconds": { "type": "integer" },
                            "staleFallback": { "type": "boolean" },
                            "enabled": { "type": "boolean" },
                            "sortOrder": { "type": "integer" }
                        },
                        "additionalProperties": true
                    }
                },
                "required": ["definition_id", "patch"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_update),
        },
        ToolEntry {
            name: "dynamic_var_delete",
            description: "Delete a dynamic variable by id.",
            schema: json!({
                "type": "object",
                "properties": { "definition_id": { "type": "string" } },
                "required": ["definition_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_delete),
        },
        ToolEntry {
            name: "dynamic_var_resolve",
            description: "Resolve dynamic variables by name: fetch from their data source when the cache is cold or expired, extract the value and return it. This really sends HTTP requests, so it needs the same trust level as api_call_endpoint.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "names": { "type": "array", "items": { "type": "string" } },
                    "environment_id": { "type": "string" },
                    "force": { "type": "boolean" }
                },
                "required": ["project_id", "names"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_resolve),
        },
        ToolEntry {
            name: "dynamic_var_set_override",
            description: "Set a dynamic variable's value by hand; it wins over the automatically fetched value until it expires or is cleared. A null value clears the override so the next resolution fetches again.",
            schema: json!({
                "type": "object",
                "properties": {
                    "definition_id": { "type": "string" },
                    "value": { "type": ["string", "null"] },
                    "environment_id": { "type": "string" },
                    "expires_at": { "type": "integer" },
                    "forever": { "type": "boolean" }
                },
                "required": ["definition_id", "value"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_set_override),
        },
    ]
}

// =========================================================================
// Handlers
// =========================================================================

async fn tool_dynamic_var_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ListArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_list args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "dynamic_var_list").await?;
    let views = crate::dynamic::ops::list(&state.pool, &a.project_id, a.environment_id.as_deref())?;
    Ok(CallToolResult::json(views))
}

async fn tool_dynamic_var_get(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: GetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_get args: {e}")))?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::project_id_of(c, &a.definition_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "dynamic_var_get").await?;
    let view =
        crate::dynamic::ops::get(&state.pool, &a.definition_id, a.environment_id.as_deref())?;
    Ok(CallToolResult::json(view))
}

async fn tool_dynamic_var_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_create args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "dynamic_var_create").await?;
    let opts = crate::dynamic::CreateOpts {
        id: a.id,
        name: a.name,
        description: a.description,
        source_endpoint_id: a.source_endpoint_id,
        source_inline_json: a.source_inline_json,
        extract_path: a.extract_path,
        value_mode: a.value_mode,
        ttl_seconds: a.ttl_seconds,
        stale_fallback: a.stale_fallback,
        enabled: a.enabled,
        sort_order: a.sort_order,
    };
    let definition = crate::dynamic::ops::create(&state.pool, &a.project_id, opts)?;
    Ok(CallToolResult::json(definition))
}

async fn tool_dynamic_var_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: UpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_update args: {e}")))?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::project_id_of(c, &a.definition_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "dynamic_var_update").await?;
    let definition = crate::dynamic::ops::update(&state.pool, &a.definition_id, a.patch)?;
    Ok(CallToolResult::json(definition))
}

async fn tool_dynamic_var_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: DeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_delete args: {e}")))?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::project_id_of(c, &a.definition_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "dynamic_var_delete").await?;
    crate::dynamic::ops::delete(&state.pool, &a.definition_id)?;
    Ok(CallToolResult::json(json!({ "deleted": a.definition_id })))
}

async fn tool_dynamic_var_resolve(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ResolveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_resolve args: {e}")))?;
    crate::mcp::check_permission(&state.pool, &a.project_id, "dynamic_var_resolve").await?;
    let resolution = crate::dynamic::ops::resolve(
        &state.pool,
        &a.project_id,
        a.environment_id.as_deref(),
        &a.names,
        a.force.unwrap_or(false),
    )
    .await?;
    Ok(CallToolResult::json(resolution))
}

async fn tool_dynamic_var_set_override(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: SetOverrideArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_set_override args: {e}")))?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::project_id_of(c, &a.definition_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "dynamic_var_set_override").await?;
    crate::dynamic::ops::set_override(
        &state.pool,
        &a.definition_id,
        a.environment_id.as_deref(),
        a.value,
        a.expires_at,
        a.forever.unwrap_or(false),
    )?;
    let view =
        crate::dynamic::ops::get(&state.pool, &a.definition_id, a.environment_id.as_deref())?;
    Ok(CallToolResult::json(view))
}

// 测试。

#[cfg(test)]
#[path = "dynamic_ops_test.rs"]
mod tests;
