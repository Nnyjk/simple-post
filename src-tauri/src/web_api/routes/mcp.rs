use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::mcp::AppState as McpAppState;
use crate::permissions::CapabilityPolicy;
use simple_post_mcp::daemon_http;

use super::super::dto::McpStatusDto;
use super::super::{map_app_error, HttpErrorResponse, WebState};

pub(crate) async fn mcp_status(State(s): State<Arc<WebState>>) -> Json<McpStatusDto> {
    Json(s.daemon.snapshot().into())
}

// =========================================================================
// MCP capability policy (commit 1: feat(mcp): three-tier capability policy)
// =========================================================================
//
// Three REST endpoints mirroring the Tauri commands in `commands.rs`:
//   GET  /api/mcp/capability       — read current policy
//   PUT  /api/mcp/capability       — set policy (body: { policy: "..." })
//   GET  /api/mcp/capability/tools — list tools visible under current policy

#[derive(Debug, Serialize)]
pub(crate) struct CapabilityResponse {
    policy: CapabilityPolicy,
}

/// `GET /api/mcp/capability` — read the global capability policy. The
/// web-mode renderer calls this on every page load so the UI badge
/// always reflects the sqlite source of truth.
pub(crate) async fn mcp_capability_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<CapabilityResponse>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let policy = match crate::db::with_conn(&pool, crate::db::preferences::mcp_policy) {
        Ok(p) => p,
        Err(e) => return Err(map_app_error(e)),
    };
    Ok(Json(CapabilityResponse { policy }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CapabilitySetBody {
    policy: String,
}

/// `PUT /api/mcp/capability` — update the global policy. Persisted via
/// `db::preferences::set_mcp_policy`, returns the new policy echoed back.
pub(crate) async fn mcp_capability_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<CapabilitySetBody>,
) -> Result<Json<CapabilityResponse>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let parsed = match CapabilityPolicy::parse(&body.policy) {
        Some(p) => p,
        None => {
            return Err(map_app_error(AppError::Validation(format!(
                "unknown mcp policy '{}' (expected read_only | safe_write | full_access)",
                body.policy
            ))))
        }
    };
    if let Err(e) =
        crate::db::with_conn(&pool, |c| crate::db::preferences::set_mcp_policy(c, parsed))
    {
        return Err(map_app_error(e));
    }
    Ok(Json(CapabilityResponse { policy: parsed }))
}

/// `GET /api/mcp/capability/tools` — list tools visible under the
/// current policy. Reuses the same `mcp::tool_descriptors` filter the
/// JSON-RPC `tools/list` path uses, so the wire output is identical.
pub(crate) async fn mcp_capability_list_tools(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Vec<String>>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let state = crate::mcp::AppState {
        pool,
        started_at: std::time::Instant::now(),
        metrics: std::sync::Arc::new(crate::mcp_metrics::McpMetrics::new()),
    };
    Ok(Json(
        crate::mcp::tool_descriptors(&state)
            .await
            .into_iter()
            .map(|t| t.name)
            .collect(),
    ))
}

pub(crate) async fn mcp_stop(
    State(s): State<Arc<WebState>>,
) -> Result<Json<&'static str>, HttpErrorResponse> {
    let _ = s.daemon.shutdown().await;
    Ok(Json("stopped"))
}

pub(crate) async fn mcp_start(
    State(s): State<Arc<WebState>>,
) -> Result<Json<McpStatusDto>, HttpErrorResponse> {
    // We don't have an `AppHandle` outside of Tauri; the daemon
    // supervisor's `ensure_started` actually only needs it for the
    // `app.path()` lookup, which has a `dirs::data_dir()` fallback in
    // `port_registry`. We construct a sentinel `AppHandle::default()`
    // is not possible — so call the data-dir-less spawn path via the
    // same helper the GUI uses.
    //
    // Implementation note: `ensure_started` requires `&AppHandle`.
    // The web shape doesn't have one. Workaround: invoke the same
    // supervisor code via a dedicated `McpDaemonHandle::ensure_started_no_app`
    // method that derives the data dir from `port_registry::data_dir()`
    // (the supervisor already stores it on the `McpDaemonHandle`).
    //
    // To keep the current call signature stable, we simply call
    // `restart` (which clears + re-spawns) — idempotent enough for
    // user "click start" semantics. If the daemon is already running,
    // `restart` will see it, kill it, and re-spawn.
    match s.daemon.restart_without_app_handle(&s.data_dir).await {
        Ok(snap) => Ok(Json(snap.into())),
        Err(e) => Err(map_app_error(e)),
    }
}

// ----- mcp daemon (additional verbs) --------------------------------------

pub(crate) async fn mcp_restart(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::commands::RestartResult>, HttpErrorResponse> {
    // Same swallow-error envelope as the Tauri command
    // (`mcp_daemon_restart`). Errors surface as `{ ok: false, pid:
    // null, port: null }` rather than as a 5xx, so the web UI's
    // restart action keeps a uniform `{ ok: bool }` contract.
    let result = match s.daemon.restart_without_app_handle(&s.data_dir).await {
        Ok(snapshot) => crate::commands::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        },
        Err(_) => crate::commands::RestartResult {
            ok: false,
            pid: None,
            port: None,
        },
    };
    Ok(Json(result))
}

pub(crate) async fn mcp_proxy_path() -> String {
    // Inline the same logic as `commands::mcp_daemon_proxy_path` so
    // we don't take a dependency on the `#[tauri::command]` macro
    // glue (which would force a `tauri::AppHandle` parameter).
    fn build(_exe_name: &str, proxy_name: &str) -> Option<std::path::PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post.exe", "simple-post-mcp.exe")
    } else {
        build("simple-post", "simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// Dispatch a JSON-RPC request to the MCP dispatcher. Wraps
/// `daemon_http::dispatch_mcp_request` (the core of the daemon's
/// `/mcp` handler) so the `serve-web` shape can serve AI clients on
/// the same port without spawning a daemon child.
pub(crate) async fn mcp_json_rpc_post(
    state: Arc<McpAppState>,
    body: String,
) -> axum::response::Response {
    daemon_http::dispatch_mcp_request(state, body).await
}

pub(crate) async fn mcp_healthz(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    use daemon_http::HealthzResponse;
    let uptime_ms = state.started_at.elapsed().as_millis() as u64;
    let requests_handled = state
        .metrics
        .requests_handled
        .load(std::sync::atomic::Ordering::Relaxed);
    let registered_tools = crate::mcp::registered_tool_names();
    axum::Json(serde_json::json!(HealthzResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms,
        requests_handled,
        current_connections: 0,
        registered_tools,
    }))
}

pub(crate) async fn mcp_tools(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    let _ = state;
    axum::Json(serde_json::json!({ "tools": crate::mcp::registered_tool_names() }))
}
