//! 变量填充 + baseUrl 解析 + 鉴权注入。
//!
//! 对应渲染进程侧的 `src/lib/url.ts`（`resolveEndpointBaseUrl`、`resolveVars`）与
//! `src/lib/http-build.ts`（`buildFetchInit`）。逻辑放在这里，MCP daemon 与 Tauri
//! 命令两条路才共用同一个解析器，而不会各自漂移。
//!
//! 两层 API：
//!
//!   * [`resolve_template`] —— 对变量表做 `{{var}}` 替换，底层积木。
//!   * [`resolve_endpoint`] —— 把 [`EndpointDetail`] + 当前 [`Environment`] +
//!     [`Project`] + 可选 [`EndpointOverrides`] 合成为可直接交给发送器的
//!     [`ResolvedEndpoint`]。
//!
//! 本文件只做门面，调用方仍写 `simple_post_mcp::variable_fill::…`：
//!
//! ```text
//!   variable_fill.rs   ← 当前文件（门面与再导出）
//!     template.rs      ← {{var}} 替换与缺变量清单（转发到 core）
//!     base_url.rs      ← baseUrl 解析 / 拼接 / 收斜杠
//!     auth.rs          ← bearer / basic / apikey / oauth2 字段替换
//!     request.rs       ← 请求头排序、query 追加、Basic 的 base64
//!     resolve.rs       ← resolve_endpoint（主 API）
//!     types.rs         ← EndpointOverrides / ResolvedEndpoint
//! ```
//!
//! `{{var}}` 的实现住在 `simple_post_core::template`：导入器要按同一套语法判定外来格式里
//! 的变量标记，而依赖方向是 `interchange → core`。此处只转发，`crate::variable_fill::` 下
//! 的路径保持不变。

mod auth;
mod base_url;
mod request;
mod resolve;
mod template;
mod types;

pub use base_url::{
    collapse_url_slashes, join_base_url, resolve_base_url, resolve_base_url_with, BaseUrlSource,
};
pub use resolve::{resolve_endpoint, resolve_endpoint_with};
pub use template::{list_missing_vars, resolve_template, ResolvedTemplate};
pub use types::{EndpointOverrides, ResolvedEndpoint};

// =========================================================================
// 测试
// =========================================================================
//
// 兄弟 `#[path]` 模块，遵循本 crate 的惯例：Rust 按模块路径找文件，所以测试专用
// *目录* 是找不到的。`template` 的测试随实现一起搬到了 core，按剩下的三块切分，
// 共用的环境 / 项目 / 端点 fixture 在 `test_support.rs`。

#[cfg(test)]
#[path = "variable_fill/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "variable_fill/base_url_test.rs"]
mod base_url_tests;

#[cfg(test)]
#[path = "variable_fill/base_url_parity_test.rs"]
mod base_url_parity_tests;

#[cfg(test)]
#[path = "variable_fill/resolve_test.rs"]
mod resolve_tests;

#[cfg(test)]
#[path = "variable_fill/resolve_parity_test.rs"]
mod resolve_parity_tests;

#[cfg(test)]
#[path = "variable_fill/auth_test.rs"]
mod auth_tests;
