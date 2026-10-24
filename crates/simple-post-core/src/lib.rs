//! Simple Post 的共享底层。
//!
//! 托管主 Tauri crate 与独立的 `simple-post-mcp` crate 双方都依赖的模块。把它们拆出来，
//! 打破了「若 `simple-post-mcp` 与 `db::` / `domain::` / `error::` 并列住在主 crate 里」
//! 本会产生的依赖环。

pub mod db;
pub mod domain;
pub mod dynamic;
pub mod error;
pub mod http_executor;
pub mod mcp_metrics;
pub mod oauth2;
pub mod permissions;
pub mod script;
pub mod template;
pub mod url;
