//! 动态变量域 MCP 工具 —— 共 7 个。
//!
//! 每个工具都是同一套四步：解析 `arguments`（小型 serde 类型）→ 过
//! `simple_post_core::permissions::require` → 委托 `crate::dynamic::ops` → 用
//! `CallToolResult::json` 包成单个 text content 块回给 LLM。
//!
//! 定义是**全局**的：不属于任何项目，任何项目的端点都能引用它，整个工作区里变量名唯一。
//! 故权限不落在「调用方在哪个项目」上，而落在**数据源接口所属的项目**上——取数会真的用到
//! 那个项目的 base URL 与鉴权。数据源是内联请求时没有项目可落，此时只由全局能力档位
//! 裁决（与 `project_list` 同理）。
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
struct GetArgs {
    definition_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateArgs {
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
    /// 取数用的环境。缺席 = 数据源端点所属项目的第一个可用环境。
    #[serde(default)]
    source_environment_id: Option<String>,
    /// 取数用的服务（`base_url_definitions.id`）。缺席 = 数据源端点所属项目的默认服务。
    #[serde(default)]
    source_base_url_definition_id: Option<String>,
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
    /// 要解析的变量名。请求体里写 `{{name}}` 用的那个。名字是全局的。
    names: Vec<String>,
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
            description: "List every dynamic variable in the workspace with its current value, cache state, and the environment and service it fetches from. Definitions are global: any project's endpoint may use them. Reads the cache only; it never fetches. It does not list the endpoints that reference a definition — query those per project with endpoint_list or endpoint_search.",
            schema: json!({
                "type": "object",
                "properties": {},
                "required": [],
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
                    "definition_id": { "type": "string" }
                },
                "required": ["definition_id"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_get),
        },
        ToolEntry {
            name: "dynamic_var_create",
            description: "Create a dynamic variable: a name, a data source (an endpoint id or an inline request), a JSONPath extraction rule and a cache TTL. The name must match ^[A-Za-z0-9_]+$ and is unique in the whole workspace. The environment and the service are optional: omitted, the fetch uses the source endpoint's first available environment and its project's default service.",
            schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "source_endpoint_id": { "type": "string" },
                    "source_inline_json": { "type": "string" },
                    "source_environment_id": { "type": "string" },
                    "source_base_url_definition_id": { "type": "string" },
                    "extract_path": { "type": "string" },
                    "value_mode": { "type": "string", "enum": ["auto", "json", "text"] },
                    "ttl_seconds": { "type": "integer" },
                    "stale_fallback": { "type": "boolean" },
                    "enabled": { "type": "boolean" },
                    "sort_order": { "type": "integer" }
                },
                "required": ["name"],
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
                            "sourceEnvironmentId": { "type": ["string", "null"] },
                            "sourceBaseUrlDefinitionId": { "type": ["string", "null"] },
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
                    "names": { "type": "array", "items": { "type": "string" } },
                    "force": { "type": "boolean" }
                },
                "required": ["names"],
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

/// 一条定义的数据源接口所属的项目。没有数据源接口（内联请求）时为 `None`。
async fn scope_of(state: &AppState, definition_id: &str) -> Result<Option<String>, AppError> {
    simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::source_project_id_of(c, definition_id)
    })
}

/// 一个接口所属的项目。查不到这个接口就是没有项目可落。
///
/// 这里丢弃的是「接口不存在」这一个错误，不是权限判定：接口不存在时随后的写入会撞上
/// 外键约束并报出来，而权限判定发生在 `require_scope` 里，用 `?` 传播。
fn project_of_endpoint(state: &AppState, endpoint_id: &str) -> Option<String> {
    let found = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, endpoint_id)
    });
    found.ok()
}

/// 按「定义的数据源所在的项目」过逐项目权限门。没有项目可落时只由全局能力档位裁决。
async fn require_scope(
    state: &AppState,
    project_id: Option<String>,
    tool_name: &str,
) -> Result<(), AppError> {
    let Some(project_id) = project_id else {
        return Ok(());
    };
    crate::mcp::check_permission(&state.pool, &project_id, tool_name).await
}

async fn tool_dynamic_var_list(state: &AppState, _args: Value) -> Result<CallToolResult, AppError> {
    // 无按项目的门：动态变量是全局的，列全部没有单一归属行可查，由 Gate A（全局
    // `CapabilityPolicy`）裁决。
    //
    // 但「哪些端点引用了它」不带走：那是别的项目的接口名与 id，而 `endpoint_list` /
    // `endpoint_search` 恰恰拒绝无项目地扫全部项目（那串接口没有单一归属项目可查门）。
    // 要看引用关系，按项目查那个项目的接口即可。
    let views = crate::dynamic::ops::list(&state.pool, false)?;
    Ok(CallToolResult::json(views))
}

async fn tool_dynamic_var_get(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: GetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_get args: {e}")))?;
    require_scope(
        state,
        scope_of(state, &a.definition_id).await?,
        "dynamic_var_get",
    )
    .await?;
    let view = crate::dynamic::ops::get(&state.pool, &a.definition_id)?;
    Ok(CallToolResult::json(view))
}

async fn tool_dynamic_var_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_create args: {e}")))?;
    // 权限落在数据源接口所属的项目上：取数会用到那个项目的 base URL 与鉴权。
    let scope = a
        .source_endpoint_id
        .as_deref()
        .and_then(|endpoint_id| project_of_endpoint(state, endpoint_id));
    require_scope(state, scope, "dynamic_var_create").await?;
    let opts = crate::dynamic::CreateOpts {
        id: a.id,
        name: a.name,
        description: a.description,
        source_endpoint_id: a.source_endpoint_id,
        source_inline_json: a.source_inline_json,
        source_environment_id: a.source_environment_id,
        source_base_url_definition_id: a.source_base_url_definition_id,
        extract_path: a.extract_path,
        value_mode: a.value_mode,
        ttl_seconds: a.ttl_seconds,
        stale_fallback: a.stale_fallback,
        enabled: a.enabled,
        sort_order: a.sort_order,
    };
    let definition = crate::dynamic::ops::create(&state.pool, opts)?;
    Ok(CallToolResult::json(definition))
}

async fn tool_dynamic_var_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: UpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_update args: {e}")))?;
    require_scope(
        state,
        scope_of(state, &a.definition_id).await?,
        "dynamic_var_update",
    )
    .await?;
    // patch 可能把数据源改指到**别的**项目的接口：那个项目的 base URL 与凭据会被用到，
    // 故那个项目的权限也得查。只查改之前的那个项目，等于给「搬走数据源」留了一条
    // 不经门的路。
    if let Some(new_endpoint_id) = a.patch.source_endpoint_id.as_ref().and_then(|v| v.as_ref()) {
        let new_scope = project_of_endpoint(state, new_endpoint_id);
        require_scope(state, new_scope, "dynamic_var_update").await?;
    }
    let definition = crate::dynamic::ops::update(&state.pool, &a.definition_id, a.patch)?;
    Ok(CallToolResult::json(definition))
}

async fn tool_dynamic_var_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: DeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_delete args: {e}")))?;
    require_scope(
        state,
        scope_of(state, &a.definition_id).await?,
        "dynamic_var_delete",
    )
    .await?;
    crate::dynamic::ops::delete(&state.pool, &a.definition_id)?;
    Ok(CallToolResult::json(json!({ "deleted": a.definition_id })))
}

async fn tool_dynamic_var_resolve(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: ResolveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_resolve args: {e}")))?;
    // 逐个名字找到它指向的定义，再按那个定义的数据源项目过门：名字是全局的，
    // 一批名字可能分属不同项目。
    for name in &a.names {
        let definition = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::dynamic_variables::get_by_name(c, name)
        })?;
        let Some(definition) = definition else {
            continue;
        };
        require_scope(
            state,
            scope_of(state, &definition.id).await?,
            "dynamic_var_resolve",
        )
        .await?;
    }
    let resolution =
        crate::dynamic::ops::resolve(&state.pool, &a.names, a.force.unwrap_or(false)).await?;
    Ok(CallToolResult::json(resolution))
}

async fn tool_dynamic_var_set_override(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: SetOverrideArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_set_override args: {e}")))?;
    require_scope(
        state,
        scope_of(state, &a.definition_id).await?,
        "dynamic_var_set_override",
    )
    .await?;
    crate::dynamic::ops::set_override(
        &state.pool,
        &a.definition_id,
        a.value,
        a.expires_at,
        a.forever.unwrap_or(false),
    )?;
    let view = crate::dynamic::ops::get(&state.pool, &a.definition_id)?;
    Ok(CallToolResult::json(view))
}

// 测试。

#[cfg(test)]
#[path = "dynamic_ops_test.rs"]
mod tests;
