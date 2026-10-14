// =========================================================================
// MCP HTTP listener configuration
// =========================================================================
// 两个 Tauri 命令。校验、持久化与「是否需要重启」的判定都在
// `simple_post_server::http_config`（与 REST 共用），桌面与 web 两种形状不会漂移——
// 为什么是这对辅助函数而不是直接写 preferences，见那个文件。
//
// 这里只剩 `#[tauri::command]` 的胶水：取出受管 pool，交过去。

use crate::db::DbPool;
use crate::domain::McpHttpConfig;
use crate::error::AppError;

use crate::http_config::McpHttpConfigDto;

/// `GET /api/mcp/http-config` —— 读 daemon 的监听配置。`preferences` 里没有这一行时
/// 回落到随包发布的 loopback 默认值而非 404，好让全新安装渲染出一个表单而不是一个错误。
#[tauri::command]
pub async fn mcp_http_config_get(
    pool: tauri::State<'_, DbPool>,
) -> Result<McpHttpConfig, AppError> {
    crate::http_config::read_http_config(pool.inner())
}

/// `PUT /api/mcp/http-config` —— 校验、持久化，并回报运行中的 daemon 是否必须重启才生效。
#[tauri::command]
pub async fn mcp_http_config_set(
    config: McpHttpConfig,
    pool: tauri::State<'_, DbPool>,
) -> Result<McpHttpConfigDto, AppError> {
    crate::http_config::write_http_config(pool.inner(), config)
}
