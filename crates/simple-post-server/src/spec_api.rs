//! 一份接口规格的编排层 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 逻辑本身住在 `simple_post_mcp::spec::ops`（MCP 工具也在调它，故三个消费方——MCP
//! 工具、REST 路由、Tauri 命令——不可能对同一件事各说各话）。本文件是 server crate
//! 这一侧的门面：Tauri 命令经它调用，REST 路由也经它调用。
//!
//! 为何不把逻辑直接写在某一侧：那样另一侧就没有对应的那一半——这正是 AGENTS.md 的
//! dual-transport invariant 要消灭的事。
//!
//! ## 不查逐项目权限档位
//!
//! `permissions` 表管的是「AI 客户端在这个项目里能做什么」，故 MCP 工具那一条路在
//! 落库前要过门。REST 路由与桌面 command 是用户自己的会话，走的是与 `nodes_create`
//! 同一套规矩 —— 那一族写路由都不查逐项目档位，这里也不查。

use simple_post_core::db::DbPool;
use simple_post_core::error::AppError;
use simple_post_core::spec::upsert::EndpointSpec;

pub use simple_post_mcp::spec::ops::SpecUpsertResult;

/// 落一份接口规格，回「落了什么」与「端点还缺什么」。
///
/// 整个落库过程在**一个事务**里：规格写到一半失败时，库里不留半截状态。
pub fn upsert(pool: &DbPool, spec: &EndpointSpec) -> Result<SpecUpsertResult, AppError> {
    simple_post_mcp::spec::ops::upsert(pool, spec)
}
