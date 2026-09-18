//! REST + static-file HTTP layer for the `simple-post serve-web` shape.
//!
//! Browser users hit this on the LAN to read / write the same SQLite
//! data the Tauri shell reads. The daemon JSON-RPC entry (`/mcp`)
//! stays untouched so MCP clients (Claude Desktop, Cursor, …) keep
//! working unchanged.
//!
//! See `docs/distribution-modes-2026-09-18.md` §6 for the route
//! table and the rationale behind the REST choice over JSON-RPC.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::services::ServeDir;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput, HttpResult};
use crate::mcp::{self, AppState as McpAppState};
use crate::port_registry;

// -------------------------------------------------------------------------
// Shared state.
// -------------------------------------------------------------------------

/// `Clone` is cheap (`DbPool` is an `Arc<Pool>` clone, the daemon
/// handle is an `Arc<Mutex<…>>` clone). The router gets one of these
/// per request via `axum::extract::State`.
#[derive(Clone)]
pub struct WebState {
    pub pool: DbPool,
    pub daemon: McpDaemonHandle,
    pub data_dir: PathBuf,
    /// MCP app state — used by the `/mcp` JSON-RPC handler so the
    /// server shape can speak the AI-client protocol directly. Same
    /// pool / metrics the daemon child uses, so a tool call from a
    /// Claude Desktop pointing at `http://server:4173/mcp` lands on
    /// the same database as the browser SPA.
    pub mcp_state: Arc<crate::mcp::AppState>,
}

// -------------------------------------------------------------------------
// Error → HTTP status mapping.
// -------------------------------------------------------------------------

/// All DB / domain errors funnel through here. JSON body keeps the
/// shape stable (`{ "code": "...", "message": "..." }`) so the
/// browser can render a toast without case-matching on status.
pub struct HttpErrorResponse {
    status: StatusCode,
    body: Json<Value>,
}

impl HttpErrorResponse {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: Json(json!({ "code": code, "message": message.into() })),
        }
    }
}

impl IntoResponse for HttpErrorResponse {
    fn into_response(self) -> Response {
        (self.status, self.body).into_response()
    }
}

fn map_app_error(e: AppError) -> HttpErrorResponse {
    // Use the existing `code()` / `Display` so we don't duplicate the
    // string-table here.
    HttpErrorResponse::new(
        match e.code() {
            "not_found" => StatusCode::NOT_FOUND,
            "conflict" => StatusCode::CONFLICT,
            "invalid_input" => StatusCode::BAD_REQUEST,
            "unauthorized" => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
        e.code(),
        e.to_string(),
    )
}

// -------------------------------------------------------------------------
// Wire DTOs (camelCase to match the TS side).
// -------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthDto {
    pub status: &'static str,
    pub version: &'static str,
    pub shape: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSendBody {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub status: String,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl From<crate::commands::McpStatusDto> for McpStatusDto {
    fn from(s: crate::commands::McpStatusDto) -> Self {
        // The Tauri command DTO already has the right shape — copy
        // field-by-field so a future rename on either side doesn't
        // silently desync.
        Self {
            status: s.status,
            running: s.running,
            pid: s.pid,
            port: s.port,
            uptime_ms: s.uptime_ms,
            requests_handled: s.requests_handled,
            current_connections: s.current_connections,
            registered_tools: s.registered_tools,
            last_error: s.last_error,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PreferencesPutBody {
    /// Single key to set. Maps directly to a `preferences` table row.
    pub key: String,
    /// JSON-encoded value. Stored verbatim by `db::preferences::set`.
    pub value: Value,
}

// -------------------------------------------------------------------------
// Handlers.
// -------------------------------------------------------------------------

async fn healthz() -> Json<HealthDto> {
    Json(HealthDto {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        shape: "server",
    })
}

async fn projects_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, |c| db::projects::list(c)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap_or_else(|_| json!([]))))
}

async fn projects_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::projects::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

async fn projects_create(
    State(s): State<Arc<WebState>>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let project: crate::domain::Project =
        serde_json::from_value(body).map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::projects::create(c, &project))
        .map_err(map_app_error)?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(project).unwrap_or(Value::Null))))
}

async fn projects_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ProjectPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` takes an `Fn` closure (not `FnOnce`), so we can't
    // move `patch` in — clone it. The struct is a small flat object.
    let row = db::with_tx(&s.pool, |c| db::projects::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

async fn projects_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::projects::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn http_send(
    State(s): State<Arc<WebState>>,
    Json(body): Json<HttpSendBody>,
) -> Result<Json<HttpResult>, HttpErrorResponse> {
    let input = HttpRequestInput {
        method: body.method,
        url: body.url,
        headers: body.headers,
        body: body.body,
        timeout_ms: body.timeout_ms,
        follow_redirects: body.follow_redirects,
        max_response_size_kb: body.max_response_size_kb,
    };
    // The function returns an `HttpResult` (success OR transport-level
    // failure wrapped in `ok=false`). Both serialise to JSON cleanly.
    Ok(Json(http_executor::execute(&input).await))
}

async fn mcp_status(
    State(s): State<Arc<WebState>>,
) -> Json<McpStatusDto> {
    Json(s.daemon.snapshot().into())
}

async fn mcp_start(
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

async fn mcp_stop(
    State(s): State<Arc<WebState>>,
) -> Result<Json<&'static str>, HttpErrorResponse> {
    s.daemon.shutdown().await;
    Ok(Json("stopped"))
}

async fn preferences_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let pairs = db::with_conn(&s.pool, |c| db::preferences::list_all(c)).map_err(map_app_error)?;
    let mut out = serde_json::Map::new();
    for (k, v) in pairs {
        // Try to parse as JSON; fall back to string. This mirrors the
        // Tauri command's `serde_json::Value` round-trip so the
        // browser sees typed values, not quoted strings.
        let parsed: Value = serde_json::from_str(&v).unwrap_or(Value::String(v));
        out.insert(k, parsed);
    }
    Ok(Json(Value::Object(out)))
}

async fn preferences_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<PreferencesPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // Re-encode the value the same way `db::preferences::set` does
    // (serde_json::to_string), so `mcp_auto_start` etc. keep parsing
    // typed values consistently.
    let raw = serde_json::to_string(&body.value)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::preferences::set(c, &body.key, &raw))
        .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------------
// Static SPA. Falls back to `index.html` so client-side routing works.
// -------------------------------------------------------------------------

/// Serves `dist/` produced by `npm run build`. The root path returns
/// `index.html`; anything under `/assets/` is served from disk via
/// `tower-http::services::ServeDir`.
async fn spa_index(spa_dir: PathBuf) -> Response {
    serve_file_or_index(spa_dir, "index.html").await
}

async fn spa_assets(
    axum::extract::State(spa_dir): axum::extract::State<PathBuf>,
    Path(rel): Path<String>,
) -> Response {
    // Reject any `..` to keep this a read-only static surface even
    // when bound to `0.0.0.0`.
    if rel.split('/').any(|seg| seg == "..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    serve_file_or_index(spa_dir, &rel).await
}

/// Read a file from `spa_dir`; on miss, return the SPA shell. Used
/// by `spa_index` (which always serves `index.html`) and by the
/// fallback handler for any unknown route.
async fn serve_file_or_index(spa_dir: PathBuf, rel: &str) -> Response {
    let path = spa_dir.join(rel);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => {
            if rel == "index.html" {
                (StatusCode::NOT_FOUND, "index.html missing — run `npm run build`")
                    .into_response()
            } else {
                // SPA fallback: serve the shell. Same async fn, but
                // the call site avoids the recursion by inlining the
                // read here — that keeps the compiler happy without
                // dragging in `Box::pin`.
                let shell = spa_dir.join("index.html");
                match tokio::fs::read(&shell).await {
                    Ok(bytes) => {
                        let mime = mime_guess::from_path(&shell).first_or_octet_stream();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, mime.as_ref())
                            .body(Body::from(bytes))
                            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                    }
                    Err(_) => (
                        StatusCode::NOT_FOUND,
                        "index.html missing — run `npm run build`",
                    )
                        .into_response(),
                }
            }
        }
    }
}

// -------------------------------------------------------------------------
// Router.
// -------------------------------------------------------------------------

/// Assemble the axum router. `spa_dir` is the **absolute** path to
/// the `dist/` directory (typically `<project_root>/dist/`); the
/// caller resolves it once and passes it in.
pub fn build_router(state: WebState, spa_dir: PathBuf) -> Router {
    let state_arc = Arc::new(state);

    let api = Router::new()
        .route("/api/healthz", get(healthz))
        .route("/api/projects", get(projects_list).post(projects_create))
        .route(
            "/api/projects/{id}",
            get(projects_get).patch(projects_update).delete(projects_delete),
        )
        .route("/api/http/send", post(http_send))
        .route("/api/mcp/status", get(mcp_status))
        .route("/api/mcp/start", post(mcp_start))
        .route("/api/mcp/stop", post(mcp_stop))
        .route("/api/preferences", get(preferences_get).put(preferences_put))
        .with_state(state_arc.clone());

    // `/mcp` JSON-RPC + `/mcp/healthz` + `/mcp/tools` — the AI client
    // entry. We don't reuse `daemon_http::build_router` because its
    // inner routes already carry the `/mcp` prefix; we'd have to
    // re-mount them under another `/mcp` (yielding `/mcp/mcp`). So
    // we register the AI-client routes here directly against the
    // shared `mcp_state` — same DB pool + metrics as the daemon
    // child uses.
    let mcp_app_state = state_arc.mcp_state.clone();
    let mcp_app_state_2 = mcp_app_state.clone();
    let mcp_app_state_3 = mcp_app_state.clone();
    let mcp_routes = Router::new()
        .route(
            "/mcp",
            post(move |body: String| mcp_json_rpc_post(mcp_app_state, body)),
        )
        .route(
            "/mcp/healthz",
            get(move || mcp_healthz(mcp_app_state_2)),
        )
        .route(
            "/mcp/tools",
            get(move || mcp_tools(mcp_app_state_3)),
        );

    // Static SPA. `ServeDir` handles `/assets/*` with proper Range /
    // ETag / Last-Modified headers; its fallback returns `index.html`
    // for client-side routing.
    let assets_dir = spa_dir.join("assets");
    let spa_clone = spa_dir.clone();
    let serve_dir = ServeDir::new(assets_dir)
        .fallback(tower_http::services::fs::ServeFile::new(spa_dir.join("index.html")));

    Router::new()
        .merge(api)
        .merge(mcp_routes)
        .fallback_service(serve_dir)
        .route("/", get(move || spa_index(spa_clone)))
}

/// Dispatch a JSON-RPC request to the MCP dispatcher. Wraps
/// `daemon_http::dispatch_mcp_request` (the core of the daemon's
/// `/mcp` handler) so the `serve-web` shape can serve AI clients on
/// the same port without spawning a daemon child.
async fn mcp_json_rpc_post(
    state: Arc<McpAppState>,
    body: String,
) -> axum::response::Response {
    crate::daemon_http::dispatch_mcp_request(state, body).await
}

async fn mcp_healthz(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    use crate::daemon_http::HealthzResponse;
    let uptime_ms = state.started_at.elapsed().as_millis() as u64;
    let requests_handled = state.metrics.requests_handled.load(std::sync::atomic::Ordering::Relaxed);
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

async fn mcp_tools(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    let _ = state;
    axum::Json(serde_json::json!({ "tools": crate::mcp::registered_tool_names() }))
}

// -------------------------------------------------------------------------
// Entry point used by `lib.rs::run_serve_web`.
// -------------------------------------------------------------------------

/// Bind a TCP listener and serve `router` until SIGINT/SIGTERM.
///
/// `data_dir` is used by the daemon supervisor to resolve the SQLite
/// file; it must match the GUI's `dirs::data_dir().join("com.simple-post")`
/// convention so the two shapes share the same database.
pub async fn serve(
    bind_addr: std::net::SocketAddr,
    data_dir: PathBuf,
    spa_dir: PathBuf,
    auto_start_daemon: bool,
) -> Result<(), AppError> {
    let pool = db::init(&data_dir)?;
    let daemon = McpDaemonHandle::new();

    if auto_start_daemon {
        // Read the persisted `mcp.auto_start` row. The CLI flag can
        // override the persisted value, but defaults to honouring it.
        let want = db::with_conn(&pool, |c| db::preferences::mcp_auto_start(c))?;
        if want {
            if let Err(e) = daemon
                .restart_without_app_handle(&data_dir)
                .await
            {
                eprintln!("[serve-web] ensure_started failed: {e:?}");
            }
        } else {
            eprintln!(
                "[serve-web] mcp.auto_start is off — daemon left stopped; \
                 use POST /api/mcp/start or the settings panel to enable."
            );
        }
    }

    // Build the MCP app state so `/mcp` JSON-RPC works in the same
    // process. The pool is shared with the GUI's `simple-post.db`;
    // metrics are fresh per process so they reset on every restart.
    let mcp_state = Arc::new(McpAppState {
        pool: pool.clone(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    });

    let state = WebState {
        pool,
        daemon,
        data_dir: data_dir.clone(),
        mcp_state,
    };
    let router = build_router(state, spa_dir);

    eprintln!("[serve-web] listening on http://{bind_addr}");
    eprintln!("[serve-web] data dir: {}", data_dir.display());
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Validation(format!("bind {bind_addr}: {e}")))?;
    axum::serve(listener, router)
        .await
        .map_err(|e| AppError::Validation(format!("axum::serve: {e}")))
}

/// Helper used by both `serve-web` and (future) embedders to resolve
/// the project root from the current executable.
///
/// `simple-post.exe` lives in `<project_root>/src-tauri/target/...`;
/// walking up two parents lands on the project root, where `dist/`
/// lives. We tolerate the absence — callers should fall back to
/// `<cwd>/dist` if the relative path doesn't exist.
pub fn default_spa_dir() -> PathBuf {
    // Walk up from the binary looking for `dist/`. Three levels is
    // enough for both `target/debug/simple-post.exe` (typical cargo
    // run) and `target/release/simple-post.exe` (CI artifacts).
    let mut here = std::env::current_exe().ok();
    for _ in 0..6 {
        let Some(p) = here.as_ref() else { break };
        let candidate = p.parent().map(|d| d.join("dist"));
        if let Some(c) = candidate {
            if c.is_dir() {
                return c;
            }
        }
        here = p.parent().map(|x| x.to_path_buf());
    }
    // Fallback: cwd/dist — works when `cargo run` is invoked from
    // the project root.
    std::env::current_dir()
        .map(|d| d.join("dist"))
        .unwrap_or_else(|_| PathBuf::from("dist"))
}

/// Helper used by both `serve-web` and (future) embedders to resolve
/// the data directory. Defaults to `dirs::data_dir()/com.simple-post`
/// via `port_registry::data_dir()`.
pub fn default_data_dir() -> Result<PathBuf, AppError> {
    port_registry::data_dir()
}

// Quiet the unused-import warning for `Method` if no per-method
// routing is added in the future. Keeping the import cheap and
// forward-compatible.
#[allow(dead_code)]
const _UNUSED_METHOD: Method = Method::GET;