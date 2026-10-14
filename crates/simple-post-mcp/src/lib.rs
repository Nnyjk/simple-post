//! MCP server module for Simple Post.
//!
//! JSON-RPC tool dispatcher + daemon HTTP transport，Tauri shell 与 `serve-web`
//! 都内嵌它；拆成独立 crate 是为了让 `simple-post-mcp` 二进制不必带上 Tauri shell。
//!
//! `daemon_http` 直接引用 `mcp::AppState` / `mcp::dispatch`，必须与 dispatcher 同
//! crate，否则形成 `simple-post` → `simple-post-mcp` → `simple-post` 环。
//! `serve-web` 经 `simple_post_mcp::daemon_http` 接入。
//!
//! `main.rs` 的独立 CLI 不自己讲 JSON-RPC，只把 stdio 帧代理给运行中的
//! `serve-web`。两侧同一套策略：全局档位存 `preferences.mcp.policy`，
//! 单个改动由 `permissions` 表按项目把守。

pub mod capability;
pub mod daemon_http;
pub mod error;
pub mod http_client;
pub mod mcp;
pub mod response_parse;
pub mod stdio_loop;
pub mod variable_fill;
