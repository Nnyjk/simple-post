//! MCP 守护进程自己那个端口上的 router。
//!
//! 它由 server crate 组装，而不是直接调 `daemon_http::serve_with_config`：MCP 的端点是
//! 唯一的对外端点，故它必须和主服务器用同一道 bearer 闸，而那道闸住在 server crate 里
//! （`simple-post-mcp` 不能依赖 server crate，依赖方向是单向的）。
//!
//! 两层：
//!
//!   * `daemon_http::build_router_with_config` —— 挂载路径（配置里的路径 + `/mcp` 别名）
//!     与 Host/Origin 守卫；
//!   * `auth::auth_middleware` —— bearer 闸。没有口令时那一层完全放行。

use std::sync::Arc;

use axum::Router;

use crate::domain::McpHttpConfig;
use crate::mcp::AppState;

/// 组装 MCP 守护进程的 router。
///
/// `password` 为 `Some` 时给除公开名单外的全部 MCP 请求加 bearer 闸。公开名单除了固定的
/// `/mcp/healthz` 之外，还包含配置里那个挂载路径对应的 `<mount>/healthz`：探针不该要求口令。
/// 空串按「没有口令」处理——一道空口令的闸会把所有请求都拒掉，而调用方以为它是开的。
pub fn build_mcp_router(
    state: Arc<AppState>,
    config: &McpHttpConfig,
    password: Option<&str>,
) -> Router {
    let router = crate::daemon_http::build_router_with_config(state, config);
    let Some(token) = password.map(str::trim).filter(|s| !s.is_empty()) else {
        return router;
    };
    let extra: Vec<String> = crate::daemon_http::mounted_prefixes(config)
        .iter()
        .map(|p| format!("{p}/healthz"))
        .collect();
    let auth_state = crate::auth::AuthState::with_public_paths(Some(token), extra);
    router.layer(axum::middleware::from_fn_with_state(
        auth_state,
        crate::auth::auth_middleware,
    ))
}

#[cfg(test)]
#[path = "mcp_router_test.rs"]
mod tests;
