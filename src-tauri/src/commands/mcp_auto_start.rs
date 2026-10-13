// ===========================================================================
// region: mcp_auto_start (preference flag)
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::error::AppError;

/// Read the persisted `mcp.auto_start` preference. The value defaults
/// to `false` (MCP daemon does NOT auto-start at launch) — the
/// rationale lives on `db::preferences::keys::MCP_AUTO_START`.
///
/// Read directly from the DB rather than via the in-memory store, so
/// the value is always the persisted truth even if the user toggled it
/// in another window since the app booted.
#[tauri::command]
pub fn mcp_auto_start_get(state: State<'_, DbPool>) -> Result<bool, AppError> {
    crate::db::with_conn(&state, crate::db::preferences::mcp_auto_start)
}

/// Persist the `mcp.auto_start` preference and immediately try to act
/// on it:
///   - `value == true`  → `start()` (idempotent; OK if already running)
///   - `value == false` → leave the daemon as-is
///
/// We do **not** shut the daemon down when the user disables
/// auto-start mid-session — `auto_start` is about whether the daemon
/// starts on launch, not a runtime kill switch. Stopping mid-flight is
/// a separate user action (`mcp_daemon_stop`, and its web twin
/// `POST /api/mcp/stop`).
///
/// The two compose like this:
///
///   * `auto_start = true`  → the daemon is up now (and up again at the
///     next launch).
///   * `auto_start = false` → only future launches are suppressed. A
///     daemon started this session stays up until the user turns the
///     manual switch off, or exits the app (`shutdown` stops it).
///   * `auto_start = false` + manual start → the daemon runs for this
///     session and does not come back on the next launch.
///
/// So "stop" is always the manual switch's job; this preference is only
/// ever about launch. `mcp_daemon_start` uses the same idempotent
/// supervisor path as this command, so enabling the toggle on an
/// already-healthy daemon reports that daemon's identity rather than
/// churning the port.
#[tauri::command]
pub async fn mcp_auto_start_set(
    state: State<'_, DbPool>,
    daemon: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
    value: bool,
) -> Result<bool, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::preferences::set(c, crate::db::preferences::keys::MCP_AUTO_START, &value)
    })?;
    if value {
        // Best-effort start; surface the error so the UI can flip the
        // toggle back if launch failed.
        daemon.start(&app_handle).await?;
    }
    Ok(value)
}
