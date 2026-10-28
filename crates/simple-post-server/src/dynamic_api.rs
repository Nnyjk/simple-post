//! 动态变量的编排层 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 逻辑本身住在 `simple_post_mcp::dynamic::ops`（MCP 工具也在调它，故三个消费方——MCP
//! 工具、REST 路由、Tauri 命令——不可能对同一件事各说各话）。本文件是 server crate 这一
//! 侧的门面：Tauri 命令经它调用，REST 路由也经它调用。
//!
//! 为何不把逻辑直接写在 Tauri 命令里：那样 web 形态就没有对应的那一半——这正是
//! AGENTS.md 的 dual-transport invariant 要消灭的事。
//!
//! 定义是**全局**的，故这些操作都不按项目作用域：名字在整个工作区里唯一，任何项目的
//! 端点都能引用同一条定义。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{
    DynamicResolution, DynamicValueStatus, DynamicVariable, DynamicVariablePatch,
};
use simple_post_core::error::AppError;

pub use simple_post_mcp::dynamic::ops::{CreateOpts, DynamicVariableView};

/// 列出全部定义，带当前值与缓存状态。
///
/// 不带「哪些端点引用了它」：那条信息只在接口那侧存在，变量这一侧不维护反向索引。
pub fn list(pool: &DbPool) -> Result<Vec<DynamicVariableView>, AppError> {
    simple_post_mcp::dynamic::ops::list(pool)
}

/// 读一条定义，带当前值与缓存状态。
pub fn get(pool: &DbPool, definition_id: &str) -> Result<DynamicVariableView, AppError> {
    simple_post_mcp::dynamic::ops::get(pool, definition_id)
}

/// 新建一条定义。
pub fn create(pool: &DbPool, opts: CreateOpts) -> Result<DynamicVariable, AppError> {
    simple_post_mcp::dynamic::ops::create(pool, opts)
}

/// 改一条定义。
pub fn update(
    pool: &DbPool,
    definition_id: &str,
    patch: DynamicVariablePatch,
) -> Result<DynamicVariable, AppError> {
    simple_post_mcp::dynamic::ops::update(pool, definition_id, patch)
}

/// 删一条定义。
pub fn delete(pool: &DbPool, definition_id: &str) -> Result<(), AppError> {
    simple_post_mcp::dynamic::ops::delete(pool, definition_id)
}

/// 解析一批名字。
pub async fn resolve(
    pool: &DbPool,
    names: &[String],
    force: bool,
) -> Result<DynamicResolution, AppError> {
    simple_post_mcp::dynamic::ops::resolve(pool, names, force).await
}

/// 强制重新取一条定义。
pub async fn refresh(pool: &DbPool, definition_id: &str) -> Result<DynamicValueStatus, AppError> {
    simple_post_mcp::dynamic::ops::refresh(pool, definition_id).await
}

/// 写覆盖值。`value` 缺席 = 清除，有值 = 写入。
///
/// 过期时间有三种可能，故线上多带一个 `forever` 开关：`forever = true` → 一直有效；
/// `expires_at = Some(ms)` → 到期时间就是它；两者都没有 → 沿用定义的 TTL。
pub fn set_override(
    pool: &DbPool,
    definition_id: &str,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: bool,
) -> Result<(), AppError> {
    simple_post_mcp::dynamic::ops::set_override(pool, definition_id, value, expires_at, forever)
}
