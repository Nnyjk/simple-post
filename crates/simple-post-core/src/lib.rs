//! Simple Post 的共享底层。
//!
//! 托管主 Tauri crate 与独立的 `simple-post-mcp` crate 双方都依赖的模块。把它们拆出来，
//! 打破了「若 `simple-post-mcp` 与 `db::` / `domain::` / `error::` 并列住在主 crate 里」
//! 本会产生的依赖环。

pub mod body;
pub mod db;
pub mod domain;
pub mod dynamic;
pub mod error;
pub mod http_executor;
pub mod oauth2;
pub mod path_params;
pub mod permissions;
/// `mcp.port` 端口文件：路径解析与记录形状。写它的一方与读它的一方分属两个 crate，
/// 故它住在两者共用的这一层。
pub mod port_file;
pub mod script;
pub mod spec;
pub mod template;
pub mod tls;
pub mod url;
