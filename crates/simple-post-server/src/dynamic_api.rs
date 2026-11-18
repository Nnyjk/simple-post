//! 动态变量的编排层 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 逻辑本身住在 `simple_post_mcp::dynamic::ops`（MCP 工具也在调它，故三个消费方——MCP 工具、
//! REST 路由、Tauri 命令——不可能对同一件事各说各话）。本文件是 server crate 这一侧的门面：
//! Tauri 命令经它调用，REST 路由也经它调用。
//! 逻辑直接写在 Tauri 命令里的话，web 形态就没有对应的那一半——这正是 dual-transport
//! invariant 要消灭的事。
//!
//! 定义挂在**项目**里：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。
//!
//! `environment_id` 参数出现的地方是**调用方此刻选中的环境**：定义没有点名取数环境
//! （继承模式）时它决定用哪个环境取数，也就决定读写哪个缓存行。缺席 = 调用方没有环境。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{
    DynamicResolution, DynamicValueStatus, DynamicVariable, DynamicVariablePatch,
};
use simple_post_core::error::AppError;

pub use simple_post_mcp::dynamic::ops::{CreateOpts, DynamicVariableView};
pub use simple_post_mcp::dynamic::preview::{PreviewDraft, PreviewIssue, PreviewResult};

/// 列出定义，带当前值与缓存状态。
///
/// `project_id` 决定列哪个范围：给了就只列那个项目里的，不给就列全部项目。
/// 不带「哪些端点引用了它」：那条信息只在接口那侧存在，变量这一侧不维护反向索引。
/// `environment_id` 只影响继承取数环境的定义：缓存行按实际生效的环境取。
pub fn list(
    pool: &DbPool,
    project_id: Option<&str>,
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
///
/// `project_id` 决定这些名字在哪个范围里找：给了就只认那个项目里的定义（请求发送走的就是
/// 这条）；不给就是跨项目取值。`environment_id` 是调用方此刻选中的环境。
pub async fn resolve(
    pool: &DbPool,
    project_id: Option<&str>,
    names: &[String],
    force: bool,
    environment_id: Option<&str>,
) -> Result<DynamicResolution, AppError> {
    simple_post_mcp::dynamic::ops::resolve(pool, project_id, names, force, environment_id).await
}

/// 强制重新取一条定义。`environment_id` 是调用方此刻选中的环境。
pub async fn refresh(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicValueStatus, AppError> {
    simple_post_mcp::dynamic::ops::refresh(pool, definition_id, environment_id).await
}

/// 写覆盖值。`value` 缺席 = 清除，有值 = 写入。
///
/// 过期时间有三种可能，故线上多带一个 `forever` 开关：`forever = true` → 一直有效；`expires_at =
/// Some(ms)` → 到期时间就是它；两者都没有 → 沿用定义的 TTL。
///
/// 覆盖值落在（定义 × 实际生效的环境）那一格，故 `environment_id` 决定写在哪：继承模式下
/// 它随调用方的选择变，不传就会写到「没有环境 / 项目默认环境」那一格去。
pub fn set_override(
    pool: &DbPool,
    definition_id: &str,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: bool,
    environment_id: Option<&str>,
) -> Result<(), AppError> {
    simple_post_mcp::dynamic::ops::set_override(
        pool,
        definition_id,
        value,
        expires_at,
        forever,
        environment_id,
    )
}

/// 用表单里当前这份草稿**真取一次数**，把响应交给人点选抽取路径。
///
/// 不碰持久状态（不读不写缓存、不过 single-flight、不写覆盖值、不写响应历史），故同一份
/// 草稿重复预览幂等。取数与真实解析共用同一个取数器，上限、超时与环境解析都是同一份实现，
/// 故「预览看得到、发送取不到」这类漂移不会发生。
///
/// `environment_id` 是**调用方此刻选中的环境**，只对继承取数环境的草稿起作用；
/// 它与 `draft.source_environment_id` 是两件事：后者是定义点名的那一个环境。
pub async fn preview(
    pool: &DbPool,
    draft: PreviewDraft,
    environment_id: Option<&str>,
) -> Result<PreviewResult, AppError> {
    simple_post_mcp::dynamic::preview::preview(pool, draft, environment_id).await
}
