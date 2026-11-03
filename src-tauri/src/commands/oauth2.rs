// ===========================================================================
// region: oauth2
// ===========================================================================

use std::collections::HashMap;

use tauri::State;

use crate::db::DbPool;
use crate::domain::AuthConfig;
use crate::error::AppError;
use crate::oauth2::{self, OAuth2ManualToken, OAuth2TokenDto, OAuth2TokenRequest};

/// 拿一份可直接注入请求的 OAuth2 token。
///
/// 薄壳：判断与取 token 全在 [`crate::oauth2`]，因为 MCP 的 `api_call_endpoint` 与
/// `serve-web` 的同名路由要走同一段逻辑。三者的入参形状共用 [`OAuth2TokenRequest`]，故 web
/// 形态下 Tauri 命令与 REST body 是同一份 JSON。
/// `force` 供 UI 的「重新获取」按钮绕过缓存；发请求的路径不传，走按需刷新。
/// `vars` 是渲染进程算好的变量池（组装请求时用的那一份）：OAuth2 配置里那四个字段的 `{{var}}`
/// 由 core 在取 token 之前替换，故渲染进程不自己替换，但必须把池送过来。
/// **不回 `refresh_token`**：它在 `oauth2_tokens` 表里续期，浏览器形态下没必要持有。
#[tauri::command]
pub async fn oauth2_token(
    state: State<'_, DbPool>,
    auth: AuthConfig,
    manual: Option<OAuth2ManualToken>,
    force: Option<bool>,
    vars: Option<HashMap<String, String>>,
) -> Result<OAuth2TokenDto, AppError> {
    oauth2::resolve(
        &state,
        &OAuth2TokenRequest {
            auth,
            manual,
            force: force.unwrap_or(false),
            vars: vars.unwrap_or_default(),
        },
    )
    .await
}
