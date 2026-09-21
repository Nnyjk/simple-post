//! Shared low-level layer for Simple Post.
//!
//! Hosts the modules that the main Tauri crate and the standalone
//! `simple-post-mcp` crate both depend on. Splitting them out breaks the
//! cycle that would otherwise arise if `simple-post-mcp` lived next to
//! `db::` / `domain::` / `error::` in the main crate (commit 3).

pub mod db;
pub mod domain;
pub mod error;
pub mod http_executor;
pub mod mcp_metrics;
pub mod permissions;