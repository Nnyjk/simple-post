//! 暴露给渲染进程的 Tauri 命令。
//!
//! 每个 `#[tauri::command]` 都是对应 `crate::db::…` 的一层薄壳；参数形状对齐
//! `src/types/domain.ts` 与 `src/lib/tauri*.ts`，线上重命名按 Tauri 2 默认走 `camelCase`。
//!
//! 取舍：
//!   * 写走 `crate::db::with_tx`，中间态不外泄；读走 `crate::db::with_conn`。
//!   * `mcp_status` 例外：它读 `crate::daemon_supervisor::McpDaemonHandle` 而非 DB，
//!     两侧共用的 DTO 见 [`McpStatusDto`]。
//!   * 共享形状放 `crate::domain`；放不进去的创建选项与移动目标放 [`dto`]。
//!
//! 布局照 `web_api/routes/`：一个资源一个模块，`dto` 放共用线上形状。子模块私有，
//! 在此 `pub use` 出去，`lib.rs` 的 `generate_handler!` 与 `web_api/` 的 REST handler
//! 都靠 `crate::commands::<name>` 解析。
//!
//! `uid` 与项目默认色定义在 `crate::domain`，`db` crate 也复用，单一定义以免 REST、
//! Tauri 命令与 SQLite 测试路径三方漂移。

mod base_url_defs;
// `pub(crate)`：REST 路由反序列化的是 Tauri 命令同一个 `CreateProjectOpts`，两个传输
// 必须对「创建带什么」达成一致，各存一份 DTO 正是本文件开头警告的漂移。
pub(crate) mod dto;
mod environments;
// 导入 / 导出的四个入口。逻辑在 `crate::interchange_api`（server crate），这里只是
// Tauri 侧的薄壳——REST 路由调的是同一批函数。
mod interchange;
mod mcp_auto_start;
mod mcp_capability;
mod mcp_http_config;
mod mcp_status;
mod node_scripts;
mod nodes;
mod oauth2;
mod permissions;
mod projects;
mod request_settings;
mod response_history;

// 必须是 glob，不能列条目：`#[tauri::command]` 除函数外还生成两个
// `#[macro_export]` 宏（`__cmd__<name>`、`__tauri_command_name_<name>`），
// 而 `generate_handler!` 把 `crate::commands::<name>` 改写成对 `crate::commands::__cmd__<name>`
// 的调用，是经该路径所在模块解析宏，而非经 crate 根的别名。列条目会把宏留在子模块，E0433 随后而来。
pub use base_url_defs::*;
pub use dto::*;
pub use environments::*;
pub use interchange::*;
pub use mcp_auto_start::*;
pub use mcp_capability::*;
pub use mcp_http_config::*;
pub use mcp_status::*;
pub use node_scripts::*;
pub use nodes::*;
pub use oauth2::*;
pub use permissions::*;
pub use projects::*;
pub use request_settings::*;
pub use response_history::*;
