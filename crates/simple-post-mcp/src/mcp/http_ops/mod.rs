//! HTTP 执行类 MCP 工具 —— 共 2 个：`api_run_request`（临时请求，不挂端点）与
//! `api_call_endpoint`（按 id 解析并发出端点）。
//!
//! 两者汇到同一条执行与结果封装路径，所以不论来源，输出形状一致。**两者也走同一个
//! 解析器**：`variable_fill::resolve_endpoint`。临时请求先被拼成一份内存里的
//! `EndpointDetail`，再交给它解析，故「变量替换 / base URL 拼接 / 鉴权派生 /
//! header 排序 / body 判空」这些规则只有一份实现 —— 不会因为「发临时请求」而另有一套
//! 拼接规则。
//!
//! ```text
//!   http_ops/
//!     mod.rs           ← 当前文件（模块地图 + 重导出）
//!     args.rs          ← 两个工具的入参形状
//!     table.rs         ← dispatch 表（两条 schema）
//!     run_request.rs   ← api_run_request
//!     call_endpoint.rs ← api_call_endpoint
//!     dynamic.rs       ← 动态变量取值
//!     send.rs          ← 发送路径、脚本环境快照、出站旋钮
//!     project.rs       ← 临时请求的项目行
//!     input.rs         ← 入参压平与解析
//!     envelope.rs      ← 结果信封
//!     multipart.rs     ← multipart 装配
//! ```

mod args;
mod call_endpoint;
mod dynamic;
mod envelope;
mod input;
mod project;
mod run_request;
mod send;
mod table;
// `crate::dynamic::source` 取数时也要用 multipart 装配，故这个模块是 crate 可见。
pub(crate) mod multipart;

// `input.rs` 与本目录下的单元断言都要用 `RunRequestArgs`，故名字提到本模块。
use args::RunRequestArgs;

pub(crate) use input::flatten_string_map;
pub(crate) use project::EMPTY_PROJECT;
pub(crate) use send::{env_snapshot, resolve_settings};
pub(crate) use table::table;

// 这两个 handler 由 `table.rs` 自己引用；本模块的这份重导出只给同目录下的测试用
// （它们按 `super::tool_api_call_endpoint` 取），故只在测试构建里存在。
#[cfg(test)]
use call_endpoint::tool_api_call_endpoint;
#[cfg(test)]
use run_request::tool_api_run_request;

// `multipart_wire_test.rs` 同样按 `super::apply` 取 multipart 装配。
#[cfg(test)]
use multipart::apply;

// =========================================================================
// 测试
// =========================================================================
//
// 兄弟 `#[path]` 模块，遵循本 crate 的惯例。`http_ops_test.rs` 是纯单元断言；
// oauth2 那组要起两个真实的 loopback socket，故它与另外几组一起住在 `http_ops/`。

#[cfg(test)]
#[path = "http_ops_test.rs"]
mod tests;

#[cfg(test)]
#[path = "dynamic_test.rs"]
mod dynamic_tests;

#[cfg(test)]
#[path = "oauth2_test.rs"]
mod oauth2_tests;

#[cfg(test)]
#[path = "multipart_mode_test.rs"]
mod multipart_mode_tests;

#[cfg(test)]
#[path = "multipart_parity_test.rs"]
mod multipart_parity_tests;

// 真实抓包：起一个 TCP 监听器，用产品自己的 `http_executor` 发一次 multipart，
// 把服务端收到的原始字节读回来解析。同目录其它用例断言的是「我们拼出的字符串长
// 什么样」，这一组断言「socket 上真的过去的是这个」。
#[cfg(test)]
#[path = "multipart_wire_test.rs"]
mod multipart_wire_tests;

#[cfg(test)]
#[path = "run_request_test.rs"]
mod run_request_tests;

#[cfg(test)]
#[path = "overrides_test.rs"]
mod overrides_tests;

#[cfg(test)]
#[path = "env_fallback_test.rs"]
mod env_fallback_tests;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_tests;

#[cfg(test)]
#[path = "tls_test.rs"]
mod tls_tests;
