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
//! See `docs/mcp-permission-policy.md` for the three-tier capability
//! policy applied in `mcp::dispatch` and `mcp::tool_descriptors`.

pub mod daemon_http;
pub mod mcp;
pub mod response_parse;
pub mod variable_fill;