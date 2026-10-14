// =========================================================================
// MCP HTTP listener configuration
// =========================================================================
//
// Two Tauri commands. The validation, persistence and restart-verdict
// logic they delegate to lives in `simple_post_server::http_config`
// (shared with the REST routes), so the desktop and web shapes cannot
// drift — see that file for why the shape is a helper pair rather than
// a raw preferences write.
//
// What is left here is only the `#[tauri::command]` glue: resolve the
// managed pool, hand it over.

use crate::db::DbPool;
use crate::domain::McpHttpConfig;
use crate::error::AppError;

use crate::http_config::McpHttpConfigDto;

/// `GET mcp_http_config` — read the daemon's listener configuration.
#[tauri::command]
pub async fn mcp_http_config_get(
    pool: tauri::State<'_, DbPool>,
) -> Result<McpHttpConfig, AppError> {
    crate::http_config::read_http_config(pool.inner())
}

/// `SET mcp_http_config` — validate, persist, and report whether the
/// running daemon has to be restarted for it to take effect.
#[tauri::command]
pub async fn mcp_http_config_set(
    config: McpHttpConfig,
    pool: tauri::State<'_, DbPool>,
) -> Result<McpHttpConfigDto, AppError> {
    crate::http_config::write_http_config(pool.inner(), config)
}
