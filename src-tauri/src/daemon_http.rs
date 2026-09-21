//! HTTP-mode MCP daemon — axum router + JSON-RPC dispatch.
//!
//! Hand-rolled JSON-RPC 2.0 over HTTP (no SSE, no batching). The
//! `simple-post-mcp-proxy` binary bridges stdio clients into HTTP for
//! in-process use; this module is what that proxy hits, plus what a
//! human curls when debugging.
//!
//! ---
//!
//! ## Why errors go in `result.is_error` (not the JSON-RPC `error` field)
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
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::mcp::{
    self, AppState, CallToolResult, JsonRpcError, JsonRpcResponse,
};

// =========================================================================
// Public router + entry
// =========================================================================

/// Build the axum [`Router`] for the MCP HTTP daemon.
///
/// Exposed so integration tests (and the eventual `daemon_supervisor`)
/// can reuse the exact same wiring the live server uses.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp_post))
        .route("/mcp/healthz", get(handle_healthz))
        .route("/mcp/tools", get(handle_tools))
        .with_state(state)
}

/// Bind + serve. The caller passes an already-bound listener so they
/// know the actual port (the `port = 0` "OS-assigned" trick).
pub async fn serve(listener: TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    let app = build_router(state);
    axum::serve(listener, app).await
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
        requests_handled: state.metrics.requests_handled.load(
            std::sync::atomic::Ordering::Relaxed,
        ),
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

async fn handle_mcp_post(
    State(state): State<Arc<AppState>>,
    body: String,
) -> Response {
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
            return Json(json_rpc_error(
                None,
                -32700,
                format!("parse error: {e}"),
            ))
            .into_response();
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
/// `{ "is_error": true }` body inside `result` rather than populating
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
        // rationale "Why errors go in `result.is_error`" at the top of
        // this file for the full reasoning.
        Err(e) => JsonRpcResponse::ok(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": format!("error: {e}"),
                }],
                "is_error": true,
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
        JsonRpcResponse::ok(
            id,
            serde_json::to_value(result).unwrap_or(Value::Null),
        )
    }
}