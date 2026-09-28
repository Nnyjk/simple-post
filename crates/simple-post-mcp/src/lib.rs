//! MCP server module for Simple Post.
//!
//! Hosts the JSON-RPC tool dispatcher plus the daemon HTTP transport that
//! the Tauri shell and `serve-web` both embed. Pulled out of the main
//! crate in commit 3 so the standalone `simple-post-mcp` binary can ship
//! without the Tauri shell.
//!
//! `daemon_http` lives here (not in the main crate) because it directly
//! references `mcp::AppState` / `mcp::dispatch`; keeping it next to the
//! dispatcher avoids a `simple-post` → `simple-post-mcp` →
//! `simple-post` cycle. `serve-web` reaches it through
//! `simple_post_mcp::daemon_http`.
//!
//! The standalone CLI binary in `main.rs` (entry point of this crate)
//! uses the same `mcp` module over HTTP: it doesn't speak JSON-RPC
//! itself, it proxies stdin/stdout frames to a running `serve-web`
//! instance. The same capability policy applies on both sides: the
//! global tier is persisted in `preferences.mcp.policy` and the
//! per-project `permissions` table gates individual mutations.
//!
//! Commit 4: real CLI replaces the placeholder stub that commit 3
//! shipped. See `crates/simple-post-mcp/README.md` for the migration
//! guide.

pub mod capability;
pub mod daemon_http;
pub mod error;
pub mod http_client;
pub mod mcp;
pub mod response_parse;
pub mod stdio_loop;
pub mod variable_fill;
