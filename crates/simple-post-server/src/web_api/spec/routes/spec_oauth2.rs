//! `/api/oauth2/token` 一组。
//!
//! 取 token 是发请求**之前**的一步，故独立成动词而不是 `/api/http/send` 的
//! 参数。返回的 `accessToken` 要由调用方拼进 `Authorization`；响应刻意不含
//! `refresh_token`，它只在服务端的 `oauth2_tokens` 表里续期。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![RouteSpec {
        method: "POST",
        path: "/api/oauth2/token",
        summary: "Resolve an OAuth2 access token for one auth config \
                      (cached, refreshed on expiry, or fetched); the caller \
                      injects it into the request it is about to send",
        stability: "public",
        request: Some("OAuth2TokenRequest"),
        response: Some("OAuth2TokenDto"),
    }]
}
