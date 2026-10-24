//! 动态变量的编排层 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 逻辑本身住在 `simple_post_mcp::dynamic::ops`（MCP 工具也在调它，故三个消费方——MCP
//! 工具、REST 路由、Tauri 命令——不可能对同一件事各说各话）。本文件是 server crate 这一
//! 侧的门面：Tauri 命令经它调用，REST 路由也经它调用。
//!
//! 为何不把逻辑直接写在 Tauri 命令里：那样 web 形态就没有对应的那一半——这正是
//! AGENTS.md 的 dual-transport invariant 要消灭的事。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{
    DynamicResolution, DynamicValueStatus, DynamicVariable, DynamicVariablePatch,
};
use simple_post_core::error::AppError;

pub use simple_post_mcp::dynamic::ops::{CreateOpts, DynamicVariableView};

/// 列出本项目的全部定义，带当前值与缓存状态。
pub fn list(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<Vec<DynamicVariableView>, AppError> {
    simple_post_mcp::dynamic::ops::list(pool, project_id, environment_id)
}

/// 读一条定义，带当前值与缓存状态。
pub fn get(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicVariableView, AppError> {
    simple_post_mcp::dynamic::ops::get(pool, definition_id, environment_id)
}

/// 新建一条定义。
pub fn create(
    pool: &DbPool,
    project_id: &str,
    opts: CreateOpts,
) -> Result<DynamicVariable, AppError> {
    simple_post_mcp::dynamic::ops::create(pool, project_id, opts)
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
    project_id: &str,
    environment_id: Option<&str>,
    names: &[String],
    force: bool,
) -> Result<DynamicResolution, AppError> {
    simple_post_mcp::dynamic::ops::resolve(pool, project_id, environment_id, names, force).await
}

/// 强制重新取一条定义。
pub async fn refresh(
    pool: &DbPool,
    project_id: &str,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicValueStatus, AppError> {
    simple_post_mcp::dynamic::ops::refresh(pool, project_id, definition_id, environment_id).await
}

/// 写覆盖值。`value` 缺席 = 清除，有值 = 写入。
///
/// 过期时间有三种可能，故线上多带一个 `forever` 开关：`forever = true` → 一直有效；
/// `expires_at = Some(ms)` → 到期时间就是它；两者都没有 → 沿用定义的 TTL。
pub fn set_override(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: bool,
) -> Result<(), AppError> {
    simple_post_mcp::dynamic::ops::set_override(
        pool,
        definition_id,
        environment_id,
        value,
        expires_at,
        forever,
    )
}
