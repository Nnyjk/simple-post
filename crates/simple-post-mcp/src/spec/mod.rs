//! 一份接口规格在 MCP crate 这一侧的部分。
//!
//! 落库的编排在 core 的 `simple_post_core::spec::upsert`；「这个端点还缺什么」的判据在
//! `crate::mcp::project_ops::endpoint::missing`，因为它要用
//! `variable_fill::resolve_base_url_with` —— 那个函数在 mcp crate 里，core 用不到它。
//!
//! 本模块把两者合成**一次调用**，并给出**一份三个宿主共用的响应形状**：MCP 工具、
//! REST 路由与桌面 command 回的是同一份 JSON，故 agent 与脚本不必各自再挖两级。
//!
//! 与 [`crate::dynamic`] 同一套分工：引擎在 core，这一侧的操作在 ops 里。

pub mod ops;

pub use ops::{upsert, SpecUpsertResult};
pub use simple_post_core::spec::upsert::{EndpointSpec, SpecOutcome};
