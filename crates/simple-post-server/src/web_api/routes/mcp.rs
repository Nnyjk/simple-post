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
// MCP HTTP listener configuration
// =========================================================================
//
//   GET /api/mcp/http-config — read bind host / port / path + allowlists
//   PUT /api/mcp/http-config — validate, persist, report restart need
//
// Both delegate to `commands::mcp_http_config`'s plain functions, so the
// REST route and the Tauri command cannot disagree about validation or
// about whether a restart is required.
//
// Note this configures the **daemon child**, not this server: the
// `/mcp` routes mounted by `web_api::build_router` stay at the canonical
// path, which is what the documented stdio-bridge → `serve-web`
// client config depends on.

/// `GET /api/mcp/http-config` — the daemon's listener configuration.
/// An absent `preferences` row answers with the shipped loopback
/// default rather than a 404, so a fresh install renders a form
/// instead of an error.
pub(crate) async fn mcp_http_config_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::domain::McpHttpConfig>, HttpErrorResponse> {
    crate::http_config::read_http_config(&s.pool)
        .map(Json)
        .map_err(map_app_error)
}

/// The `PUT` body, accepted in either of the two shapes this route can
/// legitimately receive.
///
/// * **wrapped** — `{ "config": { … } }`. This is what the dual-transport
///   `invokeCmd` sends: its `WEB_ROUTES` entry reproduces the Tauri
///   command's argument list, because one call site has to work against
///   both transports.
/// * **bare** — `{ "host": …, "path": … }`, the documented REST shape.
///
/// The wrapped form is tried first and is the only reason both can
/// coexist: `McpHttpConfig` carries `#[serde(default)]` and does not
/// deny unknown fields, so it happily accepts `{ "config": { … } }` as
/// an object whose every field is *absent* — a save that looks like it
/// worked and quietly overwrites the user's LAN config with the
/// loopback default. Ordering the variants is what stops that.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum McpHttpConfigBody {
    Wrapped {
        config: crate::domain::McpHttpConfig,
    },
    Bare(crate::domain::McpHttpConfig),
}

impl McpHttpConfigBody {
    fn into_config(self) -> crate::domain::McpHttpConfig {
        match self {
            Self::Wrapped { config } | Self::Bare(config) => config,
        }
    }
}

/// `PUT /api/mcp/http-config` — full replace. A LAN bind with
/// `allowRemote: false` is refused with a `validation` error rather
/// than stored, because that combination produces a daemon that
/// listens on the network and answers nothing.
pub(crate) async fn mcp_http_config_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<McpHttpConfigBody>,
) -> Result<Json<crate::http_config::McpHttpConfigDto>, HttpErrorResponse> {
    crate::http_config::write_http_config(&s.pool, body.into_config())
        .map(Json)
        .map_err(map_app_error)
}

// =========================================================================
// MCP capability policy
// =========================================================================
//
// Three REST endpoints mirroring the Tauri commands in `commands/`:
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

/// `POST /api/mcp/stop` — manual OFF switch.
///
/// Response envelope is the same `RestartResult` shape as
/// `/api/mcp/start` and `/api/mcp/restart`: `{ ok, pid, port }`. A
/// successful stop reports `{ ok: true, pid: null, port: null }` —
/// nothing is serving after it, so `null` is the answer, not a missing
/// field. Stopping an already-stopped daemon is `{ ok: true }`, not an
/// error.
///
/// Terminates the daemon this process spawned and one it merely
/// adopted from the port record (see `daemon_supervisor::teardown`),
/// then releases `mcp.port`.
pub(crate) async fn mcp_stop(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    match s.daemon.stop().await {
        Ok(snapshot) => Ok(Json(crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        })),
        Err(e) => {
            eprintln!("[mcp_stop] failed: {e:?}");
            Ok(Json(crate::dto::RestartResult {
                ok: false,
                pid: None,
                port: None,
            }))
        }
    }
}

/// `POST /api/mcp/start` — manual ON switch.
///
/// Idempotent, and idempotency is the point: this used to call
/// `restart_without_app_handle`, so "click start" on a healthy daemon
/// killed and respawned it — losing in-flight MCP sessions and churning
/// the port for no reason. It now goes through the supervisor's
/// idempotent `start`, which returns the existing daemon's identity
/// untouched and only spawns when nothing healthy is tracked.
///
/// Response envelope is the same `RestartResult` shape as
/// `/api/mcp/stop` and `/api/mcp/restart`: `{ ok, pid, port }`, where
/// `pid`/`port` identify the daemon now serving. Underlying failures
/// (spawn error, healthz timeout) collapse to
/// `{ ok: false, pid: null, port: null }` exactly as restart does.
pub(crate) async fn mcp_start(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    // No `AppHandle` outside of Tauri, so this is the `serve-web`
    // binding of the shared supervisor path: it derives nothing from
    // Tauri, and takes the data dir the server was started with.
    match s.daemon.start_without_app_handle(&s.data_dir).await {
        Ok(snapshot) => Ok(Json(crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        })),
        Err(e) => {
            eprintln!("[mcp_start] failed: {e:?}");
            Ok(Json(crate::dto::RestartResult {
                ok: false,
                pid: None,
                port: None,
            }))
        }
    }
}

// ----- mcp daemon (additional verbs) --------------------------------------

/// `POST /api/mcp/restart` — kill the current daemon (spawned or
/// adopted) and bring up a fresh one.
///
/// Response envelope: `RestartResult` — `{ ok, pid, port }` — byte-
/// identical to `/api/mcp/start` and `/api/mcp/stop`. Errors are
/// swallowed into `{ ok: false, pid: null, port: null }` rather than a
/// 5xx, so all three lifecycle verbs keep one uniform `{ ok: bool }`
/// contract for the UI.
pub(crate) async fn mcp_restart(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    // Same swallow-error envelope as the Tauri command
    // (`mcp_daemon_restart`). Errors surface as `{ ok: false, pid:
    // null, port: null }` rather than as a 5xx, so the web UI's
    // restart action keeps a uniform `{ ok: bool }` contract.
    let result = match s.daemon.restart_without_app_handle(&s.data_dir).await {
        Ok(snapshot) => crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        },
        Err(_) => crate::dto::RestartResult {
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
