//! 动态变量的 Tauri 命令 —— REST 路由的同一套实现的薄壳。
//!
//! 两个薄壳都只调 `crate::dynamic_api`，故桌面端与 web 形态不可能对同一件事各说各话
//! （AGENTS.md 的 dual-transport invariant）。
//!
//! **命令参数是扁平的**，不是一个 `opts: …` 包装：`#[tauri::command]` 按参数名去 payload
//! 里取值，而 REST 侧 `Json<…>` 直接吃整个 body；渲染层两边发的是同一个扁平对象，于是
//! 参数名必须就是那些 camelCase 字段名本身。
//!
//! 定义是**全局**的，故这些命令都不带项目参数。

use tauri::State;

use crate::db::DbPool;
use crate::domain::{
    DynamicResolution, DynamicValueStatus, DynamicVariable, DynamicVariablePatch, ValueMode,
};
use crate::error::AppError;

use crate::dynamic_api::DynamicVariableView;

/// 列出全部动态变量，带当前值与缓存状态。
///
/// 只读缓存行，**不取数**。
///
/// 不带「哪些端点引用了它」：那条信息只在接口那侧存在，变量这一侧不维护反向索引。
#[tauri::command]
pub fn dynamic_variables_list(state: State<DbPool>) -> Result<Vec<DynamicVariableView>, AppError> {
    crate::dynamic_api::list(&state)
}

/// 新建一条动态变量定义。
///
/// **参数多**：它们必须逐字段对应 REST body 的字段名（dual-transport invariant），
/// 故不能塞进一个 opts 结构体——那会让桌面侧的 payload 变成 `{ opts: {...} }`，
/// 与浏览器侧发出去的扁平对象对不上，只坏一端。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn dynamic_variables_create(
    state: State<DbPool>,
    name: String,
    id: Option<String>,
    description: Option<String>,
    source_endpoint_id: Option<String>,
    source_inline_json: Option<String>,
    source_environment_id: Option<String>,
    source_base_url_definition_id: Option<String>,
    extract_path: Option<String>,
    value_mode: Option<ValueMode>,
    ttl_seconds: Option<i64>,
    stale_fallback: Option<bool>,
    enabled: Option<bool>,
    sort_order: Option<i64>,
) -> Result<DynamicVariable, AppError> {
    let opts = crate::dynamic_api::CreateOpts {
        id,
        name,
        description,
        source_endpoint_id,
        source_inline_json,
        source_environment_id,
        source_base_url_definition_id,
        extract_path,
        value_mode,
        ttl_seconds,
        stale_fallback,
        enabled,
        sort_order,
    };
    crate::dynamic_api::create(&state, opts)
}

/// 改一条动态变量定义。
#[tauri::command]
pub fn dynamic_variables_update(
    state: State<DbPool>,
    id: String,
    patch: DynamicVariablePatch,
) -> Result<DynamicVariable, AppError> {
    crate::dynamic_api::update(&state, &id, patch)
}

/// 删一条动态变量定义。缓存行由外键带走。
#[tauri::command]
pub fn dynamic_variables_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::dynamic_api::delete(&state, &id)
}

/// 解析一批动态变量。这一步会真的发出站请求（去数据源取数）。
#[tauri::command]
pub async fn dynamic_variables_resolve(
    state: State<'_, DbPool>,
    names: Vec<String>,
    force: Option<bool>,
) -> Result<DynamicResolution, AppError> {
    crate::dynamic_api::resolve(&state, &names, force.unwrap_or(false)).await
}

/// 写覆盖值。`value` 缺席 = 清除，有值 = 写入。
///
/// 过期时间有三种可能，故多带一个 `forever` 开关：`forever = true` → 一直有效；
/// `expires_at = Some(ms)` → 到期时间就是它；两者都没有 → 沿用定义的 TTL。
#[tauri::command]
pub fn dynamic_variables_set_override(
    state: State<DbPool>,
    id: String,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: Option<bool>,
) -> Result<DynamicVariableView, AppError> {
    crate::dynamic_api::set_override(&state, &id, value, expires_at, forever.unwrap_or(false))?;
    crate::dynamic_api::get(&state, &id)
}

/// 强制重新取一次数。
#[tauri::command]
pub async fn dynamic_variables_refresh(
    state: State<'_, DbPool>,
    id: String,
) -> Result<DynamicValueStatus, AppError> {
    crate::dynamic_api::refresh(&state, &id).await
}
