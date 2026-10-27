//! `/api/mcp/start` 与 `/api/mcp/stop` 一组。
//!
//! 登记而非留作 internal：`start` / `stop` 是 renderer 侧的 daemon 开关，
//! 与 Tauri 命令 `mcp_daemon_start` / `mcp_daemon_stop` 同源，也是第三方
//! 客户端在运行时控制 daemon 所需的那一对。两者都回与 `/restart` 相同的
//! `{ ok, pid, port }` 信封。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "POST",
            path: "/api/mcp/start",
            summary: "Start the embedded MCP daemon (idempotent: a healthy \
                      running daemon is left untouched and echoed back)",
            stability: "public",
            request: None,
            response: Some("RestartResult"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/mcp/stop",
            summary: "Stop the embedded MCP daemon and release its port file \
                      (idempotent; always `{ ok: true, pid: null, port: null }` \
                      on success)",
            stability: "public",
            request: None,
            response: Some("RestartResult"),
        },
    ]
}
