//! `serve-web` 的 bearer 鉴权中间件。
//!
//! 默认 no-op：只有进程带非空 `SIMPLE_POST_WEB_PASSWORD`（或 CLI `--password=<token>`）时才拒绝
//! 请求。绑 `0.0.0.0` 的部署必须设，否则 `/api/*` 全网可达。
//!
//! 这一层同时用在两处：主服务器的 REST 路由树，以及 MCP 守护进程自己的 router
//! （口令经环境变量 `SIMPLE_POST_WEB_PASSWORD` 传给那个子进程）。
//!
//! 免鉴权：`/api/healthz`、`/mcp/healthz`、`/healthz`，以及 SPA shell（`GET /`）——浏览器从
//! `localStorage` 取 token，在自己的 REST 请求上带上，`<script type="module">` 带不上。
//! 守护进程另有自己的一份公开路径：配置里的挂载路径对应的 `<mount>/healthz`（见
//! [`AuthState::with_public_paths`])。
//! token 存 `localStorage` 而非 HttpOnly cookie：这里的鉴权按进程而非按用户，SSR 之前也没有别的
//! 地方能放。

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// 路由共享的鉴权状态。`password` 是 `Option<Arc<String>>`，clone 廉价。
#[derive(Clone, Default)]
pub struct AuthState {
    pub password: Option<Arc<String>>,
    /// 除 [`is_public_path`] 那份固定名单之外，这个进程额外放行的路径。
    ///
    /// 守护进程按配置里的挂载路径提供 `<mount>/healthz`，而固定名单里只有 `/mcp/healthz`
    /// 那一个别名。守护进程把配置里的那几个路径也放进这份名单，用户配了 `/ai` 时
    /// `GET /ai/healthz` 才不会被口令拦住。
    pub extra_public_paths: Arc<Vec<String>>,
}

impl AuthState {
    pub fn new(password: Option<&str>) -> Self {
        Self::with_public_paths(password, Vec::new())
    }

    /// 带一份额外的公开路径。`extra` 里的每一条按**完整路径**匹配，与
    /// [`is_public_path`] 那份固定名单同样严格（不是前缀匹配）。
    pub fn with_public_paths(password: Option<&str>, extra: Vec<String>) -> Self {
        Self {
            password: password.map(|s| Arc::new(s.to_string())),
            extra_public_paths: Arc::new(extra),
        }
    }
}

/// bearer 中间件。`state.password` 为 `None` 时完全放行；为 `Some` 时，除 [`is_public_path`]
/// 名单外的 `/api/*`、`/mcp`、`/mcp/tools` 缺少匹配的 `Authorization: Bearer <token>` 一律 401。
/// **名单成员 ≠ 路由已注册**：改名单不会注册路由，改路由也不会更新名单，两处必须一起看。
/// `/assets/*` 挂在鉴权层之外——浏览器无法给 `<script type="module">` 附 `Authorization`，挂进去
/// 则每个 JS/CSS 都 401。
pub async fn auth_middleware(
    State(state): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    // 未设密码 → 不鉴权。
    let Some(expected) = state.password.as_ref() else {
        return next.run(request).await;
    };

    // 精确匹配，避免 `/api/healthz-other` 之类被顺带放行。
    let path = request.uri().path();
    if is_public_path(path) || state.extra_public_paths.iter().any(|p| p == path) {
        return next.run(request).await;
    }

    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    // 常量时间比较，避免被时序侧信道逐字节猜出口令。
    let authed = presented
        .map(|s| constant_time_eq(s.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);

    if authed {
        next.run(request).await
    } else {
        // 带 `WWW-Authenticate`，客户端才会提示输 token 而不是空 401 循环。
        let mut response =
            (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer realm=\"simple-post\""),
        );
        response
    }
}

fn is_public_path(path: &str) -> bool {
    matches!(path, "/healthz" | "/api/healthz" | "/mcp/healthz" | "/")
}

/// 常量时间字节比较。抄自 `subtle`
/// （https://docs.rs/subtle/2.6.1/src/subtle/lib.rs.html#128-140），
/// 为一个函数不值得多拉一个依赖。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypasses_when_password_unset() {
        let state = AuthState::new(None);
        assert_eq!(state.password, None);
    }

    #[test]
    fn stores_password_when_set() {
        let state = AuthState::new(Some("hunter2"));
        assert_eq!(state.password.as_deref().unwrap().as_str(), "hunter2");
    }

    #[test]
    fn public_paths_match() {
        assert!(is_public_path("/healthz"));
        assert!(is_public_path("/api/healthz"));
        assert!(is_public_path("/mcp/healthz"));
        assert!(is_public_path("/"));
        assert!(!is_public_path("/api/projects"));
        assert!(!is_public_path("/api/projects/p_demo"));
        assert!(!is_public_path("/mcp"));
        assert!(!is_public_path("/api/healthz-other"));
    }

    #[test]
    fn constant_time_eq_handles_typical_pairs() {
        assert!(constant_time_eq(b"hunter2", b"hunter2"));
        assert!(!constant_time_eq(b"hunter2", b"Hunter2"));
        assert!(!constant_time_eq(b"hunter2", b"hunter2x"));
        assert!(!constant_time_eq(b"hunter2", b""));
        assert!(!constant_time_eq(b"", b"hunter2"));
        assert!(constant_time_eq(b"", b""));
    }
}
