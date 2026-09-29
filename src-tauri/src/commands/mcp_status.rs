// ===========================================================================
// region: mcp_status
// ===========================================================================

use tauri::State;

use crate::error::AppError;

use super::dto::{McpStatusDto, RestartResult};

/// Snapshot of the MCP daemon. Reads from the in-memory handle held
/// in Tauri state — does NOT touch the DB. The handle's `snapshot()`
/// must return [`McpStatusDto`].
#[tauri::command]
pub fn mcp_status(
    state: State<crate::daemon_supervisor::McpDaemonHandle>,
) -> Result<McpStatusDto, AppError> {
    Ok(state.snapshot())
}

/// Absolute path of the `simple-post-mcp` binary that AI clients
/// (Claude Desktop, Cursor, …) launch in their `mcpServers` config.
///
/// The renderer surfaces this string verbatim in the Connection panel's
/// "copy proxy path" button so the user can paste it into their client.
/// We resolve from `current_exe()` (the running Tauri process) and
/// substitute the trailing `simple-post(.exe)` segment with
/// `simple-post-mcp(.exe)` — the two binaries ship in the same
/// workspace `target/<profile>/` directory after `cargo build
/// --workspace`.
///
/// In a development checkout the simple-post Tauri binary lives at
/// `src-tauri/target/<profile>/simple-post(.exe)` while simple-post-mcp
/// is at the workspace root `target/<profile>/simple-post-mcp(.exe)`;
/// this command returns the same directory as `current_exe` so the
/// hint is correct for installed / packaged builds. There is no
/// npm-wrapped path, so this directory is the only one to point at.
///
/// Returns `"<unresolved current_exe>"` when the lookup fails for any
/// reason; the renderer treats that string as a non-actionable
/// placeholder rather than blocking the UI.
#[tauri::command]
pub fn mcp_daemon_proxy_path() -> String {
    use std::path::PathBuf;

    fn build(_exe_name: &str, proxy_name: &str) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post.exe", "simple-post-mcp.exe")
    } else {
        build("simple-post", "simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// Restart the embedded MCP daemon. Idempotent at the user-action
/// level — the panel disables the button while a restart is in flight
/// so two rapid clicks only result in one restart.
///
/// We swallow `AppError` into `{ ok: false, pid: null, port: null }`
/// rather than propagating it. The renderer's restart action already
/// has its own try/catch that flips the badge to `crashed` on
/// failure, so a uniform `{ ok: bool }` envelope keeps the call site
/// shape identical to the old `Promise<void>` style.
#[tauri::command]
pub async fn mcp_daemon_restart(
    state: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
) -> Result<RestartResult, AppError> {
    match state.restart(&app_handle).await {
        Ok(snapshot) => Ok(RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        }),
        Err(e) => {
            eprintln!("[mcp_daemon_restart] failed: {e:?}");
            Ok(RestartResult {
                ok: false,
                pid: None,
                port: None,
            })
        }
    }
}
