//! MCP daemon —— 顶层入口、dispatch 表与共享类型。
//!
//! ```text
//!   mcp/
//!     mod.rs            ← 当前文件（模块地图 + 重导出）
//!     protocol.rs       ← JSON-RPC 信封、工具描述符、工具函数签名
//!     state.rs          ← daemon 共享状态与启动入口
//!     registry.rs       ← 工具注册表与全局能力门
//!     dispatch.rs       ← tools/call 的分派
//!     args.rs           ← 参数辅助函数
//!     permissions.rs    ← 按项目的权限门与环境解析
//!     env_ops.rs        ← 8 个 env_* 工具
//!     key_value.rs      ← 共享的键值对 JSON Schema
//!     project_ops/      ← project / module / collection / endpoint / spec
//!     http_ops/         ← api_run_request, api_call_endpoint
//!     variable_fill.rs  ← api_resolve_template / endpoint / validate
//!     response_parse.rs ← api_parse_response / diff / extract_jsonpath
//!     dynamic_ops.rs    ← 动态变量 7 个
//! ```
//!
//! 本模块只做胶水，不定义任何域概念：域类型、DB、HTTP 执行、变量解析、响应解析、
//! 权限全在 `simple_post_core::domain` / `::db` / `::http_executor` / `::permissions`
//! 与 `crate::variable_fill` / `crate::response_parse` 里，这里只有 dispatch 接线
//! 和 JSON-RPC 信封类型。

pub mod dynamic_ops;
pub mod env_ops;
pub mod http_ops;
pub(crate) mod key_value;
pub mod project_ops;
pub mod resources;
pub mod response_parse;
pub mod variable_fill;

mod args;
mod dispatch;
mod permissions;
mod protocol;
mod registry;
mod state;

pub use args::{arg_bool, arg_bool_opt, arg_str, arg_str_opt, arg_u64, arg_u64_opt};
pub use dispatch::dispatch;
pub use permissions::check_permission;
pub use protocol::{
    CallToolContent, CallToolResult, JsonRpcError, JsonRpcResponse, ToolDescriptor, ToolFn,
};
pub use registry::{
    registered_tool_names, required_capability, tool_descriptors, REQUIRED_CAPABILITY,
};
pub use state::{serve_http, AppState};

pub(crate) use permissions::resolve_env;
pub(crate) use protocol::ToolEntry;
// `dispatch` 与 `mod_test.rs` 都按 `crate::mcp::tool_table` 这条路径取注册表。
pub(crate) use registry::tool_table;

// =========================================================================
// 工具函数装箱宏
// =========================================================================

/// 把 `async fn tool_xxx(&AppState, Value) -> Result<...>` 包成 dispatch 表
/// 里存的 boxed-future `ToolFn`。每个 `func:` 槽位都用它：
///
/// ```ignore
/// ToolEntry { name: ..., func: tool_fn!(tool_xxx), ... }
/// ```
///
/// `#[macro_export]` 把它放在 crate 根，调用处只写 `crate::tool_fn!(...)`。
/// 宏体里的路径除 `serde_json::Value` 外都带 `$crate::` 前缀，调用方无需任何
/// `use`；`serde_json::Value` 不带前缀是因为每个子模块都已经 import 了它。
#[macro_export]
macro_rules! tool_fn {
    ($f:path) => {{
        fn __boxed<'a>(
            state: &'a $crate::mcp::AppState,
            args: serde_json::Value,
        ) -> ::std::pin::Pin<
            Box<
                dyn ::std::future::Future<
                        Output = ::std::result::Result<
                            $crate::mcp::CallToolResult,
                            ::simple_post_core::error::AppError,
                        >,
                    > + ::std::marker::Send
                    + 'a,
            >,
        > {
            Box::pin($f(state, args))
        }
        __boxed
    }};
}

// =========================================================================
// 测试
// =========================================================================

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "env_resolve_test.rs"]
mod env_resolve_tests;

#[cfg(test)]
#[path = "env_switch_test.rs"]
mod env_switch_tests;

#[cfg(test)]
#[path = "resources_test.rs"]
mod resources_tests;
