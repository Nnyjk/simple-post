// ===========================================================================
// region: oauth2
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::AuthConfig;
use crate::error::AppError;
use crate::oauth2::{self, OAuth2ManualToken, OAuth2TokenDto, OAuth2TokenRequest};

/// 拿一份可直接注入请求的 OAuth2 token。
///
/// 薄壳：判断与取 token 全在 [`crate::oauth2`]，因为 MCP 的 `api_call_endpoint` 与
/// `serve-web` 的同名路由要走同一段逻辑。三者的入参形状共用
/// [`OAuth2TokenRequest`]，故 web 形态下 Tauri 命令与 REST body 是同一份 JSON。
///
/// `force` 供 UI 的「重新获取」按钮绕过缓存；发请求的路径不传，走按需刷新。
///
/// **不回 `refresh_token`**：它在 `oauth2_tokens` 表里续期，浏览器形态下没必要持有。
#[tauri::command]
pub async fn oauth2_token(
    state: State<'_, DbPool>,
    auth: AuthConfig,
    manual: Option<OAuth2ManualToken>,
    force: Option<bool>,
) -> Result<OAuth2TokenDto, AppError> {
    oauth2::resolve(
        &state,
        &OAuth2TokenRequest {
            auth,
            manual,
            force: force.unwrap_or(false),
        },
    )
    .await
}
