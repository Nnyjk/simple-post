//! 一份接口规格的编排层 —— REST 路由与 MCP 工具共用。
//!
//! 逻辑本身住在 `simple_post_mcp::spec::ops`（MCP 工具也在调它，故两个消费方——MCP 工具与
//! REST 路由——不可能对同一件事各说各话）。本文件是 server crate 这一侧的门面：
//! REST 路由经它调用。
//! 逻辑直接写在某一侧的话，另一侧就没有对应的那一半——这正是 dual-transport invariant 要
//! 消灭的事。
//!
//! 不查逐项目权限档位：`permissions` 表管的是「AI 客户端在这个项目里能做什么」，故 MCP 工具那
//! 一条路在落库前要过门。REST 路由是用户自己的会话，走的是与 `nodes_create` 同一套规矩。

use simple_post_core::db::DbPool;
use simple_post_core::error::AppError;
use simple_post_core::spec::upsert::EndpointSpec;

pub use simple_post_mcp::spec::ops::SpecUpsertResult;

/// 一份接口规格的请求体上限（字节）。
///
/// 一份规格只描述一个端点，但 `body.schema` 可以是一份很大的 JSON Schema，两段脚本也可以很长，
/// 故 2 MB 不够用。上限取 8 MB：装得下任何一份真实规格，同时仍是**有界**的缓冲。
/// 这条上限只管 REST 路由的传输层，故这个值要大到「没有任何一份真实规格会撞上它」。
pub const SPEC_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// 落一份接口规格，回「落了什么」与「端点还缺什么」。
///
/// 整个落库过程在**一个事务**里：规格写到一半失败时，库里不留半截状态。
pub fn upsert(pool: &DbPool, spec: &EndpointSpec) -> Result<SpecUpsertResult, AppError> {
    simple_post_mcp::spec::ops::upsert(pool, spec)
}
