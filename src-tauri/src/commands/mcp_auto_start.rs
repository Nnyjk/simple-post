// ===========================================================================
// region: mcp_auto_start (preference flag)
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::error::AppError;

/// 读持久化的 `mcp.auto_start`。默认 `false`（daemon 不自动启动），理由见
/// `db::preferences::keys::MCP_AUTO_START`。
///
/// 直读 DB 而非内存 store，这样即使用户在别的窗口改过，拿到的仍是持久化的真值。
#[tauri::command]
pub fn mcp_auto_start_get(state: State<'_, DbPool>) -> Result<bool, AppError> {
    crate::db::with_conn(&state, crate::db::preferences::mcp_auto_start)
}

/// 持久化 `mcp.auto_start`，并立即据此行动：`true` 走 `start()`（幂等，已在跑也算成功），
/// `false` 保持现状。
///
/// 关掉它**不会**停掉运行中的 daemon：`auto_start` 只管「启动时拉不拉」，不是运行时
/// 杀开关。现在要停是另一个用户动作（`mcp_daemon_stop`，及其 Web 对应
/// `POST /api/mcp/stop`）。手动启动的 daemon 本次会话继续跑到用户关开关或退出应用。
///
/// `mcp_daemon_start` 走的是同一条幂等 supervisor 路径，因此在已健康的 daemon 上打开
/// 开关，报的是那个 daemon 的身份，不会把端口翻来覆去地换。
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
        // 尽力拉起；错误要抛出去，UI 才能把开关翻回去。
        daemon.start(&app_handle).await?;
    }
    Ok(value)
}
