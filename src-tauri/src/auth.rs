//! Bearer-token auth middleware for the `serve-web` shape.
//!
//! The middleware is **no-op by default** — it only rejects when the
//! process was launched with a non-empty `SIMPLE_POST_WEB_PASSWORD`
//! (or `--password=<token>` on the CLI). That keeps `simple-post
//! serve-web` backwards-compatible: a developer running on
//! `127.0.0.1:4173` with no env var keeps working, but a remote
//! deployment that's bound to `0.0.0.0` *must* set a password or
//! `serve-web` rejects every `/api/*` and `/mcp` request with 401.
//!
//! Scope of the middleware:
//!   - `/api/healthz` and `/mcp/healthz` are unauthenticated (k8s
//!     liveness probes, etc.).
//!   - The SPA shell (`GET /` and any client-side route) is
//!     unauthenticated — the renderer fetches the bearer from
//!     `localStorage` (see `src/lib/tauri-commands.ts`) and attaches
//!     it on its own REST calls.
//!   - `/api/*` + `/mcp` (and `/mcp/tools`) require `Authorization:
//!     Bearer <token>` when the password is configured.
//!
//! Token storage trade-off (documented for the reviewer checklist):
//!   The frontend keeps the bearer in `localStorage` because that's
//!   the only place a server-side-rendered SPA can stash it before
//!   the bundle's JS runs. `localStorage` is readable by any XSS,
//!   which is the same exposure surface as dbx / Postman / Insomnia
//!   all accept. Not switching to a HttpOnly cookie because the
//!   auth here is per-process, not per-user.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Auth state shared across the router. Cheap to clone (inner
/// `password` is `Option<String>` and small). Clone-before-move
/// pattern matches `WebState`'s `Arc<McpAppState>` style so future
/// middleware layers can grab a clone for their own state.
#[derive(Clone, Default)]
pub struct AuthState {
    pub password: Option<Arc<String>>,
}

impl AuthState {
    pub fn new(password: Option<&str>) -> Self {
        Self {
            password: password.map(|s| Arc::new(s.to_string())),
        }
    }
}

/// Bearer-auth middleware. When `state.password` is `None` this is a
/// no-op pass-through (the entire route tree stays open). When set,
/// any `/api/*` or `/mcp` (and `/mcp/tools`) request without a
/// matching `Authorization: Bearer <token>` header returns 401.
///
/// Public-path bypass list (no auth required):
///   - `/api/healthz`  — service liveness, used by container probes
///   - `/mcp/healthz`   — same
///   - `/healthz`       — legacy, kept for backward compat
///
/// Anything else passes through. The SPA shell (`GET /` and
/// client-side routes) returns 200 + `index.html` from `serve-web`'s
/// static handler, which also lives outside the auth scope — see
/// the routing in `web_api::build_router`.
pub async fn auth_middleware(
    State(state): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    // No password configured → no auth required.
    let Some(expected) = state.password.as_ref() else {
        return next.run(request).await;
    };

    // Public path bypass. Match by exact path so a hypothetical
    // `/api/healthz-other` still requires auth.
    let path = request.uri().path();
    if is_public_path(path) {
        return next.run(request).await;
    }

    // Pull the bearer header.
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    // Constant-time comparison so a remote attacker can't time-side-channel
    // the password.
    let authed = presented
        .map(|s| constant_time_eq(s.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);

    if authed {
        next.run(request).await
    } else {
        // Add `WWW-Authenticate` so well-behaved clients prompt for a token
        // instead of looping on a blank 401.
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

/// Constant-time byte slice equality. From the `subtle` crate's docs
/// (https://docs.rs/subtle/2.6.1/src/subtle/lib.rs.html#128-140) but
/// vendored here to avoid pulling another dep just for one fn.
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
        assert!(!is_public_path("/api/healthz-other")); // hash-prefix
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
