//! project / module / collection / endpoint / baseUrlDef MCP 工具，共
//! 5 + 5 + 5 + 8 + 5 = **28 个**。
//!
//! ## 命名
//!
//! [`simple_post_core::permissions::required_permission`] 按**工具名**映射到
//! `Permission` 档位（`Read` < `ReadCall` < `Full`）。真正发外网请求的那个工具在
//! 本 crate 里叫 `api_call_endpoint`，落在 `ReadCall`。
//!
//! ## 过门策略
//!
//! 参数里直接带 `project_id` 的照传。`endpoint_get` / `_delete` / `_duplicate` /
//! `_move` / `_search` 这类只给 endpoint 或 node id 的，先查出所属项目再过门。
//!
//! ## 文件分工
//!
//! `table()` 按固定顺序把各分类的 `register(&mut v)` 拼起来，`tools/list` 的顺序
//! 因此稳定。调用方仍是 `use crate::mcp::project_ops::*` 再取 `table()`。

use serde::Deserialize;

use crate::mcp::ToolEntry;

pub(super) mod base_url_def;
pub(super) mod collection;
pub(super) mod endpoint;
pub(super) mod module;
pub(super) mod project;

// =========================================================================
// 跨子模块共用的 Args 形状
// =========================================================================
//
// `ProjectIdArgs` 被 `project_get` / `module_list` / `base_url_def_list` 共用，
// `ModuleIdArgs` 被 `collection_list` 共用。集中放在这里，各实体子模块就能
// `use super::*`，专心写自己的 Args。

#[derive(Debug, Deserialize)]
pub(super) struct ProjectIdArgs {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModuleIdArgs {
    pub module_id: String,
}

// =========================================================================
// dispatch 表
// =========================================================================

/// 拼好的 dispatch 表，顺序固定：projects (5) → modules (5) → collections (5)
/// → endpoints (8) → base_url_defs (5)。各子模块的 `register()` 只推自己那一段，
/// 这里拼起来，调用方拿到含全部 28 条的单个 `table()`。
pub(crate) fn table() -> Vec<ToolEntry> {
    let mut v = Vec::with_capacity(32);

    // ---- projects (5) ----
    project::register(&mut v);

    // ---- modules (5) ----
    module::register(&mut v);

    // ---- collections (5) ----
    collection::register(&mut v);

    // ---- endpoints (8) ----
    endpoint::register(&mut v);

    // ---- base_url_defs (5) ----
    base_url_def::register(&mut v);

    v
}

// =========================================================================
// 测试
// =========================================================================

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
