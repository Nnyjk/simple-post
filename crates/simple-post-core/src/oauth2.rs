//! OAuth2：按需取 token、必要时刷新，然后把 token 交给调用方注入请求。
//!
//! 这一层**只管 token 的生命周期**，不碰 URL、header 排序或 `{{var}}` 替换——那些是
//! `variable_fill` 与 `src/lib/http-build.ts` 的活。两边各自调 [`ensure_token`] 拿到
//! [`OAuth2Token`]，再用 [`auth_header_value`] / 各自的 query 追加把 token 挂到请求上。
//!
//! 三条不变量：
//!
//! - **token 不进日志。** 本模块与 [`crate::http_executor`] 都不打 body；`store` 的
//!   `tracing` 只记 key 前缀与到期时刻。`refresh_token` 永不离开 Rust 进程（它不进
//!   [`OAuth2TokenDto`]），浏览器的 web 形态拿不到它。
//! - **复用 [`crate::http_executor`]。** token 端点因此和普通请求走同一道
//!   [`crate::http_executor::OutboundPolicy`]（`SIMPLE_POST_BLOCK_PRIVATE_NET`）与同一套
//!   超时 / 重定向 / 体积裁剪，web + Docker 形态下它才不会成为绕过出站策略的第二条通道。
//! - **凭据只存一份。** 表 `oauth2_tokens` 只有 token；client secret 等仍只在
//!   `endpoint_details.auth_json` 里，故没有两处漂移的可能。

mod api;
mod form;
mod http_client;
mod inject;
mod parse;
mod store;
mod token_endpoint;
mod validate;

// 对外形状：定义在子模块里，在这里重新导出，故 `oauth2::resolve` 等旧路径不变。
pub use api::{
    ensure_token, fetch_and_store, resolve, save_manual, OAuth2ManualToken, OAuth2TokenDto,
    OAuth2TokenRequest,
};
pub use inject::{auth_header_value, auth_query_value, goes_in_query, inject};
pub use store::auth_key;
pub use validate::validate;

pub use crate::db::oauth2_tokens::OAuth2Token;

use crate::oauth2::parse::parse_pasted;

/// 提前多久把 token 当成已过期。
///
/// 宁可早刷新也不晚：晚意味着拿一个刚失效的 token 发出请求，服务端回 401，用户看到
/// 一次莫名失败。60 秒够盖住「取回来 → 组装 → 发出」这段，也够盖住双方时钟的一点偏差。
const EXPIRY_SKEW_SECS: i64 = 60;

/// token 端点的超时。它不是用户的业务请求，不该继承 `settings.timeoutMs`。
const TOKEN_TIMEOUT_MS: u64 = 30_000;

/// token 响应体积上限（KB）。token 响应只有几百字节；1 MB 的异常响应（IdP 的错误页）
/// 只配拿到一句可读的第一行，不必整页塞进内存。
const TOKEN_MAX_RESPONSE_KB: u64 = 64;

/// IdP 的 `error_description` 最多摘多少字符进错误信息。错误页经常是一整张 HTML，
/// 全量拼进 `AppError` 会顶掉真正有用的那句。
const MAX_ERROR_DETAIL: usize = 300;

#[cfg(test)]
mod test_fixtures;

#[cfg(test)]
#[path = "oauth2/unit_test.rs"]
mod unit_tests;

#[cfg(test)]
#[path = "oauth2/inject_parity_test.rs"]
mod inject_parity_tests;

#[cfg(test)]
#[path = "oauth2/live_test.rs"]
mod live_tests;

#[cfg(test)]
#[path = "oauth2/resolve_test.rs"]
mod resolve_tests;
