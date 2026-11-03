//! `POST /api/oauth2/token` —— 取一份可直接注入请求的 OAuth2 token。
//!
//! 与 Tauri 的 `oauth2_token` 命令共用 `simple_post_core::oauth2::resolve`，故两种
//! 形态对同一份 `AuthConfig` 得同一份结果；入参形状也共用
//! [`simple_post_core::oauth2::OAuth2TokenRequest`]，渲染层一次调用即可，不必分叉。
//!
//! **受 bearer 鉴权层保护**：它会向一个由 `auth.tokenUrl` 指定的地址发请求，而 URL 由
//! 请求体决定。把它放进 `auth::is_public_path` 等于给未鉴权的调用方开一条出站通道。
//!
//! **body 里的 `vars` 是变量池**：渲染进程组装请求时算出的那一份（`buildPool(env,
//! dynamicVars)`）。OAuth2 配置里那四个字段的 `{{var}}` 由 core 在取 token 之前替换
//! （规则见 `simple_post_core::oauth2`），故渲染进程不自己替换，但必须把池送过来 ——
//! 送的是**同一份**池，否则同一个 `{{var}}` 在 URL 里能解析、在 `tokenUrl` 里不能解析。
//! 缺席 `vars` 时配置里的 `{{var}}` 原样留着，取 token 会报出缺哪个变量。

use std::sync::Arc;

use axum::{extract::State, Json};

use crate::error::AppError;
use crate::oauth2::{self, OAuth2TokenDto, OAuth2TokenRequest};

use super::super::{map_app_error, HttpErrorResponse, WebState};

pub(crate) async fn oauth2_token(
    State(s): State<Arc<WebState>>,
    Json(body): Json<OAuth2TokenRequest>,
) -> Result<Json<OAuth2TokenDto>, HttpErrorResponse> {
    // 错误不静默降级成「空 token」：取不到 token 就该让这次请求根本发不出去，
    // 否则发出去的是一条裸奔的请求，而用户以为它带鉴权。
    oauth2::resolve(&s.pool, &body)
        .await
        .map(Json)
        .map_err(map_error)
}

fn map_error(e: AppError) -> HttpErrorResponse {
    map_app_error(e)
}
