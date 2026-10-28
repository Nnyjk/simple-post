//! 动态变量域 MCP 工具 —— 共 7 个。
//!
//! 每个工具都是同一套四步：解析 `arguments`（小型 serde 类型）→ 过
//! `simple_post_core::permissions::require` → 委托 `crate::dynamic::ops` → 用
//! `CallToolResult::json` 包成单个 text content 块回给 LLM。
//!
//! 定义挂在**项目**里：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。故权限
//! 落在两处：**归属项目**（谁拥有这条定义）与**数据源接口所属的项目**（取数会真的用到
//! 那个项目的 base URL 与鉴权）。数据源是内联请求时没有第二个项目可落。
//!
//! `dynamic_var_resolve` 会真的发出站请求，故它的 capability 是 `Send`（与
//! `api_call_endpoint` 同级），逐项目权限是 `ReadCall`。它可以跨项目取值：点名
//! `project_id` 就是那个项目里的定义，不点名就是全部项目——同名定义出现在多个项目里
//! 时记一条「歧义」错误，不猜。

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
struct ListArgs {
    /// 只列这个项目里的定义。缺席 = 列出全部项目。
    #[serde(default)]
    project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateArgs {
    /// 归属项目。定义挂在项目里，故它必填。
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
    /// 取数用的环境。缺席 = 数据源端点所属项目的第一个可用环境。
    #[serde(default)]
    source_environment_id: Option<String>,
    /// 取数用的服务（`base_url_definitions.id`）。缺席 = 数据源接口所属项目的默认服务。
    #[serde(default)]
    source_base_url_definition_id: Option<String>,
    /// JSONPath 表达式。`$` 表示整段响应。
    #[serde(default)]
    extract_path: Option<String>,
    /// 处理脚本。抽取之后跑，用来做 JSONPath 表达不了的加工。脚本拿得到抽取结果
    /// （`pm.data`）与原始响应（`pm.response`），用 `return` 给出最终值。
    #[serde(default)]
    transform_script: Option<String>,
    /// 值的序列化形态：`auto` / `json` / `text`。
    #[serde(default)]
    value_mode: Option<ValueMode>,
    #[serde(default)]
    ttl_seconds: Option<i64>,
    /// 取数失败时是否允许用已过期的缓存值回退。
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
    /// 要解析的变量名。请求体里写 `{{name}}` 用的那个。
    names: Vec<String>,
    /// 在哪个项目里找这些名字。缺席 = 跨项目找（同名定义出现在多个项目里时按歧义
    /// 处理，不猜）。
    #[serde(default)]
    project_id: Option<String>,
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
            description: "List dynamic variables with their current value and cache state. Pass project_id for one project, omit it for every project. Reads the cache only; it never fetches.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" }
                },
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
            description: "Create a dynamic variable in a project: a name, a data source (an endpoint id or an inline request), a JSONPath extraction rule, a transform script and a cache TTL. The name must match ^[A-Za-z0-9_]+$ and is unique within the project. Omit the environment and the service to use the source endpoint's first environment and its project's default service.",
            schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "source_endpoint_id": { "type": "string" },
                    "source_inline_json": { "type": "string" },
                    "source_environment_id": { "type": "string" },
                    "source_base_url_definition_id": { "type": "string" },
                    "extract_path": { "type": "string" },
                    "transform_script": { "type": "string" },
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
                            "projectId": { "type": "string" },
                            "name": { "type": "string" },
                            "description": { "type": ["string", "null"] },
                            "sourceEndpointId": { "type": ["string", "null"] },
                            "sourceInlineJson": { "type": ["string", "null"] },
                            "sourceEnvironmentId": { "type": ["string", "null"] },
                            "sourceBaseUrlDefinitionId": { "type": ["string", "null"] },
                            "extractPath": { "type": "string" },
                            "transformScript": { "type": ["string", "null"] },
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
            description: "Resolve dynamic variables by name: fetch from their data source when the cache is cold or expired, extract the value, run the optional transform script and return it. Pass project_id for one project, omit it for all projects, where a name in several projects is ambiguous. This really sends HTTP requests, so it needs the trust level of api_call_endpoint.",
            schema: json!({
                "type": "object",
                "properties": {
                    "names": { "type": "array", "items": { "type": "string" } },
                    "project_id": { "type": "string" },
                    "force": { "type": "boolean" }
                },
                "required": ["names"],
                "additionalProperties": false,
            }),
            func: tool_fn!(tool_dynamic_var_resolve),
        },
        ToolEntry {
            name: "dynamic_var_set_override",
            description: "Set a dynamic variable's value by hand; it wins over the fetched value until it expires or is cleared. A null value clears the override.",
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

/// 一条定义的**归属项目**。未归属的定义（迁移遗留，等用户认领）为 `None`。
async fn owner_of(state: &AppState, definition_id: &str) -> Result<Option<String>, AppError> {
    simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::dynamic_variables::project_id_of(c, definition_id)
    })
}

/// 一条定义的**数据源接口所属的项目**。数据源是内联请求时为 `None`。
async fn source_scope_of(
    state: &AppState,
    definition_id: &str,
) -> Result<Option<String>, AppError> {
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

/// 按项目过逐项目权限门。没有项目可落时只由全局能力档位裁决。
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

async fn tool_dynamic_var_list(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: ListArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_list args: {e}")))?;
    // 列全部项目时不按项目设门（与 `project_list` 同理）：那串定义没有单一的归属行可查。
    // 点了名要列某个项目时，那个项目的读权限就是那道门。
    if let Some(project_id) = a.project_id.as_deref() {
        require_scope(state, Some(project_id.to_string()), "dynamic_var_list").await?;
    }
    let views = crate::dynamic::ops::list(&state.pool, a.project_id.as_deref())?;
    Ok(CallToolResult::json(views))
}

async fn tool_dynamic_var_get(state: &AppState, args: Value) -> Result<CallToolResult, AppError> {
    let a: GetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("dynamic_var_get args: {e}")))?;
    require_scope(
        state,
        owner_of(state, &a.definition_id).await?,
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
    // 归属项目：定义写进那个项目，故那个项目的权限决定这次写入。
    require_scope(state, Some(a.project_id.clone()), "dynamic_var_create").await?;
    // 数据源接口所属的项目：取数会用到那个项目的 base URL 与鉴权。
    let source_scope = a
        .source_endpoint_id
        .as_deref()
        .and_then(|endpoint_id| project_of_endpoint(state, endpoint_id));
    require_scope(state, source_scope, "dynamic_var_create").await?;
    let opts = crate::dynamic::CreateOpts {
        id: a.id,
        project_id: Some(a.project_id),
        name: a.name,
        description: a.description,
        source_endpoint_id: a.source_endpoint_id,
        source_inline_json: a.source_inline_json,
        source_environment_id: a.source_environment_id,
        source_base_url_definition_id: a.source_base_url_definition_id,
        extract_path: a.extract_path,
        transform_script: a.transform_script,
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
    // 改之前的归属项目。
    require_scope(
        state,
        owner_of(state, &a.definition_id).await?,
        "dynamic_var_update",
    )
    .await?;
    // 改之后的归属项目：patch 可能把它搬到别的项目去。
    if let Some(new_owner) = a.patch.project_id.as_deref() {
        require_scope(state, Some(new_owner.to_string()), "dynamic_var_update").await?;
    }
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
        owner_of(state, &a.definition_id).await?,
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
    // 逐个名字找到它指向的定义，再过两道门：归属项目（谁拥有这条定义）与数据源项目
    // （取数会花谁的钱）。一批名字可能分属不同项目。
    for name in &a.names {
        let definitions = match a.project_id.as_deref() {
            Some(project_id) => simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::dynamic_variables::get_by_name_in_project(c, project_id, name)
            })?
            .into_iter()
            .collect::<Vec<_>>(),
            None => simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::dynamic_variables::get_by_name_across(c, name)
            })?,
        };
        for definition in definitions {
            require_scope(
                state,
                owner_of(state, &definition.id).await?,
                "dynamic_var_resolve",
            )
            .await?;
            require_scope(
                state,
                source_scope_of(state, &definition.id).await?,
                "dynamic_var_resolve",
            )
            .await?;
        }
    }
    let resolution = crate::dynamic::ops::resolve(
        &state.pool,
        a.project_id.as_deref(),
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
    require_scope(
        state,
        owner_of(state, &a.definition_id).await?,
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
