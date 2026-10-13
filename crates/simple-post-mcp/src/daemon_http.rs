//! HTTP-mode MCP daemon — axum router + JSON-RPC dispatch.
//!
//! Hand-rolled JSON-RPC 2.0 over HTTP (no SSE, no batching). The
//! `simple-post-mcp-proxy` binary bridges stdio clients into HTTP for
//! in-process use; this module is what that proxy hits, plus what a
//! human curls when debugging.
//!
//! ---
//!
//! ## Why errors go in `result.isError` (not the JSON-RPC `error` field)
//!
//! **JSON-RPC 2.0 recap.** The spec defines two error channels:
//! `error` is for *transport / envelope* failures (parse error -32700,
//! invalid request -32600, method not found -32601, invalid params
//! -32602, internal error -32603). A successful `result` carries
//! whatever the invoked method returned. The two are mutually exclusive
//! in a single response.
//!
//! **MCP sits on top of JSON-RPC.** Per the MCP spec, `tools/call`
//! responses use `result.content` for success and `result.isError: true`
//! for tool-level failure — i.e. the tool *ran*, but its work reported
//! an error. The LLM is still expected to read the body so it can
//! surface the message to the user and / or recover.
//!
//! **Our choice.** Every tool-handler failure (both
//! "the-tool-itself-failed" and "transport-of-the-tool blew up mid-way")
//! is routed into a synthetic `result.isError: true` body. It never
//! uses the JSON-RPC `error` field. Envelope-level failures (malformed
//! JSON, missing `jsonrpc: "2.0"`, unknown method, missing
//! `params.name`) DO still flow through `error` via the parsing arm
//! above the dispatch site — those happen before any method body runs
//! and have no tool body for the LLM to read, so the spec channel is
//! the only honest one.
//!
//! **Reasoning.** LLM clients (Claude Desktop, Cursor, our own proxy)
//! treat tool errors uniformly: show the message to the user, retry, or
//! skip. They do not branch on transport-vs-tool to decide their UX.
//! Collapsing both flavours into a single observable channel
//! (`result.isError`) keeps the LLM's recovery loop simple — read
//! `result.content`, decide.
//!
//! **Trade-off accepted.** Callers that *do* want to distinguish
//! transport-level from tool-level failures (e.g. a future metrics
//! dashboard that wants to alert on deserialization bugs separately
//! from genuine tool crashes) lose that signal at the JSON-RPC
//! boundary. We accept this because the metrics surface that actually
//! needs the split lives one layer down on `GET /mcp/healthz`
//! (`requests_handled` and the server-side counter for envelope
//! failures), where the distinction is still observable. The JSON-RPC
//! response body is not the right place to re-introduce it.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use simple_post_core::domain::{McpHttpConfig, DEFAULT_MCP_PATH};
use tokio::net::TcpListener;

use crate::mcp::{self, AppState, CallToolResult, JsonRpcError, JsonRpcResponse};

// =========================================================================
// Public router + entry
// =========================================================================

/// The three request paths for one mount prefix, as
/// `(json_rpc, healthz, tools)`.
///
/// Pure and separate from [`mcp_routes`] on purpose: axum 0.7's
/// `Router` exposes no route listing, so a unit test asserting which
/// paths got mounted has to be able to ask that question without
/// standing up a server.
pub fn route_paths(path: &str) -> (String, String, String) {
    (
        path.to_string(),
        format!("{path}/healthz"),
        format!("{path}/tools"),
    )
}

/// Every prefix the router serves: the configured one, plus the
/// canonical `/mcp` when it differs.
///
/// The alias is not optional decoration.
/// `port_registry::healthz` probes a hardcoded
/// `http://127.0.0.1:<port>/mcp/healthz`, so a user who renames the
/// path without it would get a daemon the supervisor reports as
/// `Unhealthy` — and `ensure_started` fails on outright — while the
/// daemon itself is serving perfectly. It also keeps every
/// already-configured AI client (which hardcodes `/mcp`; see the
/// "MCP client must not hardcode a daemon port" note in AGENTS.md)
/// working across the rename.
pub fn mounted_prefixes(config: &McpHttpConfig) -> Vec<String> {
    let path = config.normalized_path();
    if path == DEFAULT_MCP_PATH {
        return vec![path];
    }
    vec![path, DEFAULT_MCP_PATH.to_string()]
}

/// The three AI-client routes, mounted at `path`.
///
/// `path` must already be normalised (leading `/`, no trailing one) —
/// call it through [`McpHttpConfig::normalized_path`].
fn mcp_routes(path: &str) -> Router<Arc<AppState>> {
    let (json_rpc, healthz, tools) = route_paths(path);
    Router::new()
        .route(&json_rpc, post(handle_mcp_post))
        .route(&healthz, get(handle_healthz))
        .route(&tools, get(handle_tools))
}

/// Build the axum [`Router`] for the MCP HTTP daemon with the shipped
/// defaults (loopback bind, `/mcp`, remote access off).
///
/// Exposed so integration tests (and the eventual `daemon_supervisor`)
/// can reuse the exact same wiring the live server uses. The daemon
/// binary calls [`build_router_with_config`] instead — this wrapper
/// exists for the default case and for tests that don't care about
/// listener configuration.
pub fn build_router(state: Arc<AppState>) -> Router {
    build_router_with_config(state, &McpHttpConfig::default())
}

/// Build the router honouring the user's `mcp.http_config`.
///
/// The mount prefixes come from [`mounted_prefixes`] (configured path
/// plus the `/mcp` alias); the allowlists are installed as a middleware
/// by [`host_origin_guard`], whose default is loopback-only —
/// fail-closed, because the alternative reading of "no allowlist" is
/// "allow everything", which turns the remote switch itself into the
/// vulnerability.
pub fn build_router_with_config(state: Arc<AppState>, config: &McpHttpConfig) -> Router {
    mounted_prefixes(config)
        .into_iter()
        .fold(Router::new(), |acc, prefix| acc.merge(mcp_routes(&prefix)))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(config.clone()),
            host_origin_guard,
        ))
        .with_state(state)
}

/// Bind + serve with the default configuration. The caller passes an
/// already-bound listener so they know the actual port (the `port = 0`
/// "OS-assigned" trick).
pub async fn serve(listener: TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    serve_with_config(listener, state, McpHttpConfig::default()).await
}

/// Bind + serve on an already-bound listener under the user's
/// `mcp.http_config`.
pub async fn serve_with_config(
    listener: TcpListener,
    state: Arc<AppState>,
    config: McpHttpConfig,
) -> std::io::Result<()> {
    let app = build_router_with_config(state, &config);
    axum::serve(listener, app).await
}

// =========================================================================
// Host / Origin guard
// =========================================================================
//
// Mounted over the whole MCP router. The rules, in the order they are
// applied:
//
//   1. `allow_remote == false` (the default): the `Host` header must
//      resolve to loopback, or equal the configured bind host.
//   2. `allow_remote == true`: a non-loopback `Host` must match
//      `allowedHosts` (a bare `*` matches anything). **An empty list
//      denies every non-loopback host** — reading "no allowlist" as
//      "allow everything" would turn the switch itself into the
//      vulnerability.
//   3. `Origin`: absent means an AI client (curl, Claude Code, Claude
//      Desktop) and is allowed; present must match `allowedOrigins`.
//
// Loopback is allowed in every configuration, including with a
// populated allowlist. That is what keeps `port_registry::healthz`
// working after the user configures a LAN-only allowlist.

/// Strip the port off a `Host` header value.
///
/// Returns the bare host name so the allowlist compares names, not
/// `name:port` pairs — a client reaching the daemon on `:4173` must
/// still match an entry the user typed as `studio.local`.
pub fn host_name(raw: &str) -> String {
    let h = raw.trim();
    if let Some(rest) = h.strip_prefix('[') {
        // Bracketed IPv6 literal, with or without `:port`. A bare
        // IPv6 address is not a legal Host header, so an unterminated
        // bracket is passed through as-is and simply won't match.
        if let Some(close) = rest.find(']') {
            return rest[..close].to_string();
        }
        return h.to_string();
    }
    match h.rsplit_once(':') {
        // Only strip a numeric tail: a bare `::1` has several colons
        // and its "port" is not digits.
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            name.to_string()
        }
        _ => h.to_string(),
    }
}

/// Does one allowlist entry cover `value`?
///
/// A bare `*` is the wildcard. Host names compare case-insensitively
/// (DNS is case-insensitive).
fn entry_matches(pattern: &str, value: &str) -> bool {
    let p = pattern.trim();
    p == "*" || p.eq_ignore_ascii_case(value)
}

/// The `Host` half of the guard. `None` denies: a request that carries
/// no `Host` is not something this daemon should be guessing about.
pub fn host_permitted(config: &McpHttpConfig, host_header: Option<&str>) -> bool {
    let Some(raw) = host_header else {
        return false;
    };
    let host = host_name(raw);
    if simple_post_core::domain::is_loopback_host(&host) {
        return true;
    }
    // The configured bind host is always itself acceptable — otherwise
    // `allow_remote` on a named interface would reject its own traffic.
    if host == host_name(&config.host) {
        return true;
    }
    if !config.allow_remote {
        return false;
    }
    config.allowed_hosts.iter().any(|p| entry_matches(p, &host))
}

/// The `Origin` half of the guard.
///
/// `None` allows. AI clients issue plain HTTP requests with no
/// `Origin` at all, so requiring one would break every real caller;
/// the header only shows up for browser-shaped requests, which is
/// exactly the case worth checking.
pub fn origin_permitted(config: &McpHttpConfig, origin: Option<&str>) -> bool {
    let Some(o) = origin else {
        return true;
    };
    config
        .allowed_origins
        .iter()
        .any(|p| p.trim() == "*" || p.trim() == o.trim())
}

/// Combined decision, shaped so both halves are testable without an
/// HTTP client and the middleware stays a thin adapter.
pub fn check_access(
    config: &McpHttpConfig,
    host_header: Option<&str>,
    origin: Option<&str>,
) -> Result<(), String> {
    if !host_permitted(config, host_header) {
        return Err(format!(
            "host {:?} is not allowed (allowRemote: {}, allowedHosts: {:?})",
            host_header.unwrap_or("<none>"),
            config.allow_remote,
            config.allowed_hosts
        ));
    }
    if !origin_permitted(config, origin) {
        return Err(format!(
            "origin {:?} is not in allowedOrigins ({:?})",
            origin.unwrap_or("<none>"),
            config.allowed_origins
        ));
    }
    Ok(())
}

/// 403 in the same envelope shape the rest of the backend uses —
/// `{ "error": { "code": "forbidden", "message": "..." } }`, with
/// `code` the stable contract and `message` the human-facing part.
fn forbidden_response(message: String) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": {
                "code": "forbidden",
                "message": message,
            }
        })),
    )
        .into_response()
}

async fn host_origin_guard(
    State(config): State<Arc<McpHttpConfig>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    match check_access(&config, host, origin) {
        Ok(()) => next.run(request).await,
        Err(message) => forbidden_response(message),
    }
}

// =========================================================================
// `GET /mcp/healthz`
// =========================================================================

async fn handle_healthz(State(state): State<Arc<AppState>>) -> Json<HealthzResponse> {
    let uptime_ms = state.started_at.elapsed().as_millis() as u64;
    Json(HealthzResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms,
        requests_handled: state
            .metrics
            .requests_handled
            .load(std::sync::atomic::Ordering::Relaxed),
        current_connections: 0, // axum 0.7 doesn't surface this cheaply
        registered_tools: mcp::registered_tool_names(),
    })
}

#[derive(Debug, Serialize)]
pub struct HealthzResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
}

// =========================================================================
// `GET /mcp/tools`
// =========================================================================

async fn handle_tools(State(state): State<Arc<AppState>>) -> Json<ToolsListResponse> {
    Json(ToolsListResponse {
        tools: mcp::tool_descriptors(&state).await,
    })
}

#[derive(Debug, Serialize)]
pub struct ToolsListResponse {
    pub tools: Vec<mcp::ToolDescriptor>,
}

// =========================================================================
// `POST /mcp` — JSON-RPC 2.0 entry point
// =========================================================================

async fn handle_mcp_post(State(state): State<Arc<AppState>>, body: String) -> Response {
    dispatch_mcp_request(state, body).await
}

/// Core of `handle_mcp_post` — exposed so the `serve-web` shape can
/// reuse the exact same JSON-RPC dispatch under the same `/mcp`
/// route, without re-mounting the daemon router (which would
/// double-prefix `/mcp`).
pub async fn dispatch_mcp_request(state: Arc<AppState>, body: String) -> Response {
    state
        .metrics
        .requests_handled
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return Json(json_rpc_error(None, -32700, format!("parse error: {e}"))).into_response();
        }
    };

    if request.jsonrpc != "2.0" {
        return Json(json_rpc_error(
            request.id,
            -32600,
            "jsonrpc must be \"2.0\"".to_string(),
        ))
        .into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id, request.params),
        "ping" => JsonRpcResponse::ok(request.id, json!({})),
        "tools/list" => handle_tools_list(state.clone(), request.id).await,
        "tools/call" => handle_tools_call(state, request.id, request.params).await,
        "notifications/initialized" | "notifications/cancelled" => {
            return StatusCode::NO_CONTENT.into_response();
        }
        other => json_rpc_error(request.id, -32601, format!("method not found: {other}")),
    };

    Json(response).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    id: Option<Value>,
}

fn handle_initialize(id: Option<Value>, _params: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::ok(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "simple-post-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )
}

async fn handle_tools_list(state: Arc<AppState>, id: Option<Value>) -> JsonRpcResponse {
    let tools = mcp::tool_descriptors(&state).await;
    JsonRpcResponse::ok(id, json!({ "tools": tools }))
}

/// Handle a `tools/call` JSON-RPC request.
///
/// Tool-level failures are surfaced by embedding a synthetic
/// `{ "isError": true }` body inside `result` rather than populating
/// the top-level `error` field. Rationale: per the MCP spec, an LLM
/// must always be able to *read* a failed tool's message so it can
/// recover and retry. A transport-level `error` short-circuits before
/// the LLM sees the body and would block recovery, so we deliberately
/// don't use it here. Real transport-level problems (parse errors,
/// unknown methods) still go through the `error` arm via
/// `json_rpc_error`.
async fn handle_tools_call(
    state: Arc<AppState>,
    id: Option<Value>,
    params: Option<Value>,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return json_rpc_error(id, -32602, "missing params".to_string()),
    };
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n.to_string(),
        None => return json_rpc_error(id, -32602, "params.name (string) required".to_string()),
    };
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    match mcp::dispatch(&state, &name, args).await {
        Ok(result) => JsonRpcResponse::ok_with_result(id, result),
        // Tool-handler failure → embed in `result.isError: true` rather
        // than the JSON-RPC `error` field. See the module-level
        // rationale "Why errors go in `result.isError`" at the top of
        // this file for the full reasoning.
        //
        // Hand-written JSON, NOT a `CallToolResult` — so the spec's
        // `isError` spelling has to be typed out here as well. `CallToolResult`
        // gets it right via `#[serde(rename_all = "camelCase")]`; this arm
        // bypasses that, and shipped `is_error` while the struct was fixed.
        Err(e) => JsonRpcResponse::ok(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": format!("error: {e}"),
                }],
                "isError": true,
            }),
        ),
    }
}

fn json_rpc_error(id: Option<Value>, code: i64, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id: id.unwrap_or(Value::Null),
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}

// =========================================================================
// small helper: render an already-serialized CallToolResult as JSON-RPC
// =========================================================================

/// Used only by tests + the `tools/call` path. Renamed from `ok` so we
/// can take a strongly-typed `CallToolResult` (and the existing `ok`
/// still accepts a raw `Value` for `tools/list`, `ping`, etc.).
pub trait JsonRpcResponseExt {
    fn ok_with_result(id: Option<Value>, result: CallToolResult) -> Self;
}

impl JsonRpcResponseExt for JsonRpcResponse {
    fn ok_with_result(id: Option<Value>, result: CallToolResult) -> Self {
        JsonRpcResponse::ok(id, serde_json::to_value(result).unwrap_or(Value::Null))
    }
}

// =========================================================================
// Tests
// =========================================================================
//
// Host / Origin allowlist behaviour and the router's path mounting live
// in the sibling file so they can never reach for a socket.
#[cfg(test)]
#[path = "daemon_http_test.rs"]
mod tests;
