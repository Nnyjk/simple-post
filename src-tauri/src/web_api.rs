//! REST + static-file HTTP layer for the `simple-post serve-web` shape.
//!
//! Browser users hit this on the LAN to read / write the same SQLite
//! data the Tauri shell reads. The daemon JSON-RPC entry (`/mcp`)
//! stays untouched so MCP clients (Claude Desktop, Cursor, …) keep
//! working unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::services::ServeDir;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput, HttpResult};
use crate::mcp::AppState as McpAppState;
use crate::permissions::CapabilityPolicy;
use crate::port_registry;
// `daemon_http` and `variable_fill` / `response_parse` moved into the
// `simple-post-mcp` crate in commit 3 (MCP layer extraction).
use simple_post_mcp::daemon_http;

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
    /// Optional bearer password. `None` keeps the route tree
    /// unauthenticated (commit-5 backwards-compatible default).
    /// `Some(_)` enforces the `auth_middleware` on every `/api/*`
    /// and `/mcp` request. The frontend caches the bearer in
    /// `localStorage` and attaches it via `webFetch()`; see
    /// `docs/http-api-reference.md` for the contract.
    pub password: Option<String>,
    /// Optional reverse-proxy base path (e.g. `"/sp"` for an
    /// nginx-prefixed deployment). When set, the entire router is
    /// `nest`ed under this prefix; `curl /api/projects` returns 404
    /// and `curl /sp/api/projects` returns 200.
    pub base_path: Option<String>,
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
// Wire helpers — kept tiny, but server-injected fields all flow through
// here so the SPA vs REST contract stays 1:1 with the Tauri command path.
// -------------------------------------------------------------------------

/// Fill in a server-generated `id` when the caller (SPA) omits one.
/// Mirrors what every `commands.rs::*_create` does: take the user-facing
/// fields, then stamp `id = uid()`. Without this, every REST create
/// fails with `[validation] missing field 'id'` because the domain
/// structs all require `id` while the SPA only ships name / description
/// / color / etc.
fn ensure_server_id(body: &mut Value) {
    let needs_fill = match body.get("id") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if needs_fill {
        body["id"] = json!(crate::domain::uid());
    }
}

// -------------------------------------------------------------------------
// Wire DTOs for the resource CRUD surface. Keep this block small —
// every shape that's already in `domain::*` is reused directly so
// the wire contract stays 1:1 with the TS side.
// -------------------------------------------------------------------------

/// Body for `POST /api/endpoints`. Mirrors [`domain::Endpoint`] but
/// omits the timestamps — the server fills them with `now_ms()` so
/// callers don't have to know the millisecond-clock invariant.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointWrite {
    /// Optional — server fills via `ensure_server_id` when omitted.
    #[serde(default)]
    pub id: String,
    pub collection_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub method: crate::domain::HttpMethod,
    pub url: String,
    #[serde(default)]
    pub environment_id: Option<String>,
    #[serde(default)]
    pub base_url_definition_id: Option<String>,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub params: Vec<crate::domain::KeyValue>,
    #[serde(default)]
    pub headers: Vec<crate::domain::KeyValue>,
    #[serde(default)]
    pub body: crate::domain::RequestBody,
    #[serde(default)]
    pub auth: crate::domain::AuthConfig,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub sort_order: i64,
}

/// Body for `POST /api/endpoints/search`. All fields optional —
/// omitted filters don't constrain the result. Mirrors the args of
/// `db::endpoints::search` (q, method, tags) so the same SQL runs.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EndpointsSearchBody {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Body for `POST /api/endpoints/move`. `endpoint_ids` is the
/// caller-ordered list (drag-and-drop preserves selection order).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointMoveBody {
    pub endpoint_ids: Vec<String>,
    pub collection_id: String,
    pub index: i32,
}

/// Body for `POST /api/collections/move`. `parent_kind` is one of
/// `"module"` (reparent to top-level under a module) or
/// `"collection"` (reparent to be a child of a sibling collection).
/// Unknown values surface as `AppError::Validation` from the DB
/// layer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMoveBody {
    pub collection_ids: Vec<String>,
    pub parent_kind: String,
    pub parent_id: String,
    pub index: i32,
}

/// Body for `POST /api/modules/move`. `target_index` is the zero-based
/// slot the moved module lands at; clamping happens in the DB layer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleMoveBody {
    pub module_id: String,
    pub target_index: i32,
}

/// Body for `POST /api/environments/:envId/vars`. `value = null`
/// deletes the variable (mirrors `db::environments::set_var`'s
/// "None removes the key" semantics).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetVarBody {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Body for `POST /api/environments/:envId/baseurls`. `url = null`
/// removes the slot.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetBaseUrlBody {
    pub def_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// Body for `POST /api/environments/active`. `envId` lives in the
/// body (not the URL) so the literal route `/active` can act as the
/// "set the active environment" verb without colliding with
/// `/api/environments/:envId`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveEnvBody {
    pub env_id: String,
}

/// Body for `PUT /api/permissions/:projectId`. The `permission`
/// string is parsed via [`domain::Permission::parse`] — a typo at the
/// call site fails with `AppError::Validation` instead of writing a
/// bogus row.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPutBody {
    pub permission: String,
}

/// Body for `POST /api/projects/:projectId/baseurls/default`.
/// `def_id = null` clears the project's default pointer.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefSetDefaultBody {
    #[serde(default)]
    pub def_id: Option<String>,
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

async fn projects_list(State(s): State<Arc<WebState>>) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
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
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let project: crate::domain::Project = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::projects::create(c, &project)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&project).unwrap_or(Value::Null)),
    ))
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
    State(_s): State<Arc<WebState>>,
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

async fn mcp_status(State(s): State<Arc<WebState>>) -> Json<McpStatusDto> {
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
struct CapabilityResponse {
    policy: CapabilityPolicy,
}

/// `GET /api/mcp/capability` — read the global capability policy. The
/// web-mode renderer calls this on every page load so the UI badge
/// always reflects the sqlite source of truth.
async fn mcp_capability_get(
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
struct CapabilitySetBody {
    policy: String,
}

/// `PUT /api/mcp/capability` — update the global policy. Persisted via
/// `db::preferences::set_mcp_policy`, returns the new policy echoed back.
async fn mcp_capability_put(
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
async fn mcp_capability_list_tools(
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

async fn mcp_stop(State(s): State<Arc<WebState>>) -> Result<Json<&'static str>, HttpErrorResponse> {
    let _ = s.daemon.shutdown().await;
    Ok(Json("stopped"))
}

async fn preferences_get(State(s): State<Arc<WebState>>) -> Result<Json<Value>, HttpErrorResponse> {
    let pairs = db::with_conn(&s.pool, db::preferences::list_all).map_err(map_app_error)?;
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
    db::with_tx(&s.pool, |c| db::preferences::set(c, &body.key, &raw)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------------
// Resource CRUD handlers — one block per resource, mirrors the
// `region:` sections in `commands.rs`. Bodies come in three flavours:
//   1. List  — GET with `?projectId=` / `?moduleId=` / `?collectionId=`
//   2. Write — POST (full create) or PATCH (partial update) or DELETE
//   3. Action — POST to a sub-resource (`/move`, `/:id/duplicate`,
//              `/:envId/vars`, …) — body carries the args.
// -------------------------------------------------------------------------

// ----- modules ------------------------------------------------------------

async fn modules_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Filter by project when given; otherwise fall back to `list_all`
    // so the SPA bootstrap path (no active project yet) still gets a
    // useful dataset to filter client-side.
    let rows = match params.get("projectId") {
        Some(project_id) => db::with_conn(&s.pool, |c| db::modules::list_by_project(c, project_id))
            .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::modules::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

async fn modules_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    // Accept the full `Module` struct. `Module` has no server-side
    // timestamp fields so `#[serde(default)]` on those isn't needed —
    // but `id` is server-stamped via `ensure_server_id` so the SPA can
    // ship name/description/sortOrder/etc. without a hand-rolled id.
    let module: crate::domain::Module = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::modules::create(c, &module)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&module).unwrap_or(Value::Null)),
    ))
}

async fn modules_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ModulePatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` takes an `Fn` closure (not `FnOnce`), so we can't
    // move `patch` in — clone it. Same idiom as `projects_update`.
    let row = db::with_tx(&s.pool, |c| db::modules::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn modules_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::modules::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn modules_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<ModuleMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        // `r#move` is the raw identifier (`move` is a Rust keyword).
        db::modules::r#move(c, &body.module_id, body.target_index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- collections --------------------------------------------------------

async fn collections_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("moduleId") {
        Some(module_id) => {
            db::with_conn(&s.pool, |c| db::collections::list_by_module(c, module_id))
                .map_err(map_app_error)?
        }
        None => db::with_conn(&s.pool, db::collections::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

async fn collections_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let collection: crate::domain::Collection = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::collections::create(c, &collection)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&collection).unwrap_or(Value::Null)),
    ))
}

async fn collections_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::CollectionPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::collections::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn collections_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::collections::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn collections_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<CollectionMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::collections::move_many(
            c,
            &body.collection_ids,
            &body.parent_kind,
            &body.parent_id,
            body.index,
        )
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- endpoints ----------------------------------------------------------

async fn endpoints_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Filter by collection when given; otherwise `list_all`. We don't
    // honour `projectId`/`moduleId` here — those would need additional
    // db overloads; callers that want project- or module-scoped walks
    // can filter the unfiltered list client-side until a richer db
    // function lands.
    let rows = match params.get("collectionId") {
        Some(collection_id) => db::with_conn(&s.pool, |c| {
            db::endpoints::list_by_collection(c, collection_id)
        })
        .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::endpoints::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

async fn endpoints_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::endpoints::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn endpoints_search(
    State(s): State<Arc<WebState>>,
    Json(body): Json<EndpointsSearchBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `tags.as_deref()` lets the DB layer accept `&[String]` without
    // moving the `Vec` out of the body.
    let tags_slice = body.tags.as_deref();
    let rows = db::with_conn(&s.pool, |c| {
        db::endpoints::search(c, body.query.as_deref(), body.method.as_deref(), tags_slice)
    })
    .map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
}

async fn endpoints_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    // `EndpointWrite` covers the caller-side fields; the server fills
    // `id` (when omitted), `created_at`, and `updated_at`. We deserialize
    // as `EndpointWrite` for typed safety of nested structs (params,
    // headers, body, auth) and then promote to the full `Endpoint` to
    // hand off to the db layer.
    ensure_server_id(&mut body);
    let write: EndpointWrite = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    let now = crate::domain::now_ms();
    let endpoint = crate::domain::Endpoint {
        id: write.id,
        collection_id: write.collection_id,
        name: write.name,
        description: write.description,
        method: write.method,
        url: write.url,
        environment_id: write.environment_id,
        base_url_definition_id: write.base_url_definition_id,
        base_url_override: write.base_url_override,
        params: write.params,
        headers: write.headers,
        body: write.body,
        auth: write.auth,
        docs: write.docs,
        notes: write.notes,
        tags: write.tags,
        sort_order: write.sort_order,
        created_at: now,
        updated_at: now,
    };
    db::with_tx(&s.pool, |c| db::endpoints::create(c, &endpoint)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&endpoint).unwrap_or(Value::Null)),
    ))
}

async fn endpoints_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::EndpointPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::endpoints::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn endpoints_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::endpoints::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn endpoints_duplicate(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<String>, HttpErrorResponse> {
    let new_id =
        db::with_tx(&s.pool, |c| db::endpoints::duplicate(c, &id)).map_err(map_app_error)?;
    Ok(Json(new_id))
}

async fn endpoints_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<EndpointMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::endpoints::move_many(c, &body.endpoint_ids, &body.collection_id, body.index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- environments -------------------------------------------------------

async fn environments_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("projectId") {
        Some(project_id) => db::with_conn(&s.pool, |c| {
            db::environments::list_by_project(c, project_id)
        })
        .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::environments::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

async fn environments_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let env: crate::domain::Environment = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::environments::create(c, &env)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&env).unwrap_or(Value::Null)),
    ))
}

async fn environments_update(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(patch): Json<crate::domain::EnvironmentPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::update(c, &env_id, patch.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn environments_delete(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::environments::delete(c, &env_id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn environments_set_var(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(body): Json<EnvSetVarBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::set_var(c, &env_id, &body.key, body.value.as_deref())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn environments_set_baseurl(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(body): Json<EnvSetBaseUrlBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::set_base_url(c, &env_id, &body.def_id, body.url.as_deref())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn environments_set_active(
    State(s): State<Arc<WebState>>,
    Json(body): Json<SetActiveEnvBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::environments::set_active(c, &body.env_id))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

// ----- baseUrlDefs --------------------------------------------------------
//
// `base_url_defs` live in their own table; we keep them reachable via
// two route families:
//   - `/api/projects/:projectId/baseurls[/default]` for project-scoped
//     creates and "set as project default" actions
//   - `/api/baseurl-defs/:defId` for id-scoped updates / deletes
//
// This mirrors the path the resource lives on in the renderer (a def
// "belongs to" a project) without forcing the TS caller to remember
// which table a delete hits.

/// Read the project's base-URL definitions.
///
/// The renderer does not normally call this: `Project` nests its
/// `baseUrlDefinitions`, so the store already has them after bootstrap.
/// It exists so a REST client that just created or patched a definition
/// can read the family back — previously the family offered POST / PATCH
/// / DELETE / set-default but no way to list.
async fn base_url_defs_list(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, |c| {
        db::base_url_defs::list_by_project(c, &project_id)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&rows).unwrap_or(Value::Null)))
}

async fn base_url_defs_create(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let def: crate::domain::BaseUrlDefinition = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::base_url_defs::create(c, &def, &project_id))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&def).unwrap_or(Value::Null)),
    ))
}

async fn base_url_defs_update(
    State(s): State<Arc<WebState>>,
    Path(def_id): Path<String>,
    Json(patch): Json<crate::commands::BaseUrlDefPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `BaseUrlDefPatch` lives in `commands.rs` (not `domain`) because
    // it has no other callers. Reusing it here keeps the wire shape
    // identical to the IPC path.
    let row = db::with_tx(&s.pool, |c| {
        db::base_url_defs::update(c, &def_id, patch.name.clone(), patch.description.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

async fn base_url_defs_delete(
    State(s): State<Arc<WebState>>,
    Path(def_id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::base_url_defs::delete(c, &def_id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn base_url_defs_set_default(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefSetDefaultBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Echo back the post-update project so the renderer can replace
    // its in-memory copy without an extra round-trip.
    let row = db::with_tx(&s.pool, |c| {
        db::base_url_defs::set_default(c, &project_id, body.def_id.as_deref())?;
        crate::db::projects::get(c, &project_id)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

// ----- request settings (singleton row in preferences KV) -----------------

/// Preference key under which the singleton `RequestSettings` is
/// stored. Same key as the Tauri command — the two shapes share the
/// row.
const REQUEST_SETTINGS_KEY: &str = "request_settings";

async fn request_settings_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Missing row → `unwrap_or_default()`. Matches the Tauri command's
    // "always returns a value" contract so the renderer can call this
    // unconditionally.
    let stored: Option<crate::domain::RequestSettings> =
        db::with_conn(&s.pool, |c| db::preferences::get(c, REQUEST_SETTINGS_KEY))
            .map_err(map_app_error)?;
    let settings = stored.unwrap_or_default();
    Ok(Json(serde_json::to_value(&settings).unwrap_or(Value::Null)))
}

async fn request_settings_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<crate::domain::RequestSettings>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // PUT semantics: full replace. Unlike the Tauri command's
    // `RequestSettingsPatch`, we accept the entire struct and write
    // it back verbatim. Callers wanting partial updates can fall
    // back to `/api/preferences` (`{"key": "request_settings", "value": ...}`).
    db::with_tx(&s.pool, |c| {
        db::preferences::set(c, REQUEST_SETTINGS_KEY, &body)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&body).unwrap_or(Value::Null)))
}

// ----- permissions --------------------------------------------------------

async fn permissions_get(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
) -> Result<Json<String>, HttpErrorResponse> {
    // Mirror the Tauri command: return the bare `Permission` (which
    // serializes to a lowercase string via
    // `#[serde(rename_all = "lowercase")]`). The TS caller already
    // has this contract on the IPC side.
    let perm =
        db::with_conn(&s.pool, |c| db::permissions::get(c, &project_id)).map_err(map_app_error)?;
    Ok(Json(perm.as_str().to_string()))
}

async fn permissions_set(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<PermissionPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // `Permission::parse` is the single source of truth for the
    // accepted values — a typo at the call site fails loudly with
    // 400 instead of writing a bogus row.
    let perm = crate::domain::Permission::parse(&body.permission).ok_or_else(|| {
        map_app_error(AppError::Validation(format!(
            "unknown permission: {}",
            body.permission
        )))
    })?;
    db::with_tx(&s.pool, |c| db::permissions::set(c, &project_id, perm)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- mcp daemon (additional verbs) --------------------------------------

async fn mcp_restart(
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

async fn mcp_proxy_path() -> String {
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

// -------------------------------------------------------------------------
// Static SPA. Falls back to `index.html` so client-side routing works.
// -------------------------------------------------------------------------

/// Serves `dist/` produced by `npm run build`. The root path returns
/// `index.html`; anything under `/assets/` is served from disk via
/// `tower-http::services::ServeDir`.
async fn spa_index(spa_dir: PathBuf) -> Response {
    serve_file_or_index(spa_dir, "index.html").await
}

#[allow(dead_code)] // reserved for future asset-serve route; not yet wired in build_router
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
                (
                    StatusCode::NOT_FOUND,
                    "index.html missing — run `npm run build`",
                )
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
///
/// `state.password` (when `Some`) installs the bearer middleware
/// over every route except the documented public bypass list (see
/// `auth::is_public_path`). `state.base_path` (when `Some`) `nest`s
/// the entire router under that prefix so a reverse proxy can mount
/// the SPA at e.g. `https://host/sp/`. Both fields are `None` by
/// default — same behaviour as before commit 5.
pub fn build_router(state: WebState, spa_dir: PathBuf) -> Router {
    // Pull the auth fields before `Arc::new` consumes the struct —
    // commit 5 split them out so a future `auth_middleware` consumer
    // can read them without holding the heavy state.
    let password = state.password.clone();
    let base_path = state.base_path.clone();
    let state_arc = Arc::new(state);

    let api = Router::new()
        .route("/api/healthz", get(healthz))
        // ---- projects (already existed) ----
        .route("/api/projects", get(projects_list).post(projects_create))
        .route(
            "/api/projects/:id",
            get(projects_get)
                .patch(projects_update)
                .delete(projects_delete),
        )
        .route("/api/http/send", post(http_send))
        // ---- modules ----
        .route("/api/modules", get(modules_list).post(modules_create))
        // `/move` is registered before `/:id` so the literal path
        // takes precedence in the router's radix tree.
        .route("/api/modules/move", post(modules_move))
        .route(
            "/api/modules/:id",
            patch(modules_update).delete(modules_delete),
        )
        // ---- collections ----
        .route(
            "/api/collections",
            get(collections_list).post(collections_create),
        )
        .route("/api/collections/move", post(collections_move))
        .route(
            "/api/collections/:id",
            patch(collections_update).delete(collections_delete),
        )
        // ---- endpoints ----
        .route("/api/endpoints", get(endpoints_list).post(endpoints_create))
        // Action verbs (`/search`, `/move`) are registered before
        // `/:id` so the literals win over the parameter.
        .route("/api/endpoints/search", post(endpoints_search))
        .route("/api/endpoints/move", post(endpoints_move))
        .route(
            "/api/endpoints/:id",
            get(endpoints_get)
                .patch(endpoints_update)
                .delete(endpoints_delete),
        )
        .route("/api/endpoints/:id/duplicate", post(endpoints_duplicate))
        // ---- environments ----
        .route(
            "/api/environments",
            get(environments_list).post(environments_create),
        )
        // `/active` (literal) before `/:envId` (param) for the same
        // radix-tree precedence reason.
        .route("/api/environments/active", post(environments_set_active))
        .route(
            "/api/environments/:envId",
            patch(environments_update).delete(environments_delete),
        )
        .route("/api/environments/:envId/vars", post(environments_set_var))
        .route(
            "/api/environments/:envId/baseurls",
            post(environments_set_baseurl),
        )
        // ---- baseUrlDefs ----
        // Project-scoped create lives under the project; id-scoped
        // update/delete uses a separate `/baseurl-defs/:defId`
        // namespace so a future cross-project lookup doesn't have to
        // re-key on `projectId`.
        .route(
            "/api/projects/:projectId/baseurls",
            get(base_url_defs_list).post(base_url_defs_create),
        )
        .route(
            "/api/projects/:projectId/baseurls/default",
            post(base_url_defs_set_default),
        )
        .route(
            "/api/baseurl-defs/:defId",
            patch(base_url_defs_update).delete(base_url_defs_delete),
        )
        // ---- request settings (singleton) ----
        .route(
            "/api/request-settings",
            get(request_settings_get).put(request_settings_put),
        )
        // ---- permissions (per project) ----
        .route(
            "/api/permissions/:projectId",
            get(permissions_get).put(permissions_set),
        )
        // ---- mcp (existing + new verbs) ----
        .route("/api/mcp/status", get(mcp_status))
        .route("/api/mcp/start", post(mcp_start))
        .route("/api/mcp/stop", post(mcp_stop))
        .route("/api/mcp/restart", post(mcp_restart))
        .route("/api/mcp/proxy-path", get(mcp_proxy_path))
        // ---- mcp capability policy (commit 1) ----
        .route(
            "/api/mcp/capability",
            get(mcp_capability_get).put(mcp_capability_put),
        )
        .route("/api/mcp/capability/tools", get(mcp_capability_list_tools))
        // ---- preferences ----
        .route(
            "/api/preferences",
            get(preferences_get).put(preferences_put),
        )
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
        .route("/mcp/healthz", get(move || mcp_healthz(mcp_app_state_2)))
        .route("/mcp/tools", get(move || mcp_tools(mcp_app_state_3)));

    // Static SPA. `ServeDir` handles `/assets/*` with proper Range /
    // ETag / Last-Modified headers; its fallback returns `index.html`
    // for client-side routing.
    //
    // The root is `spa_dir` itself, NOT `spa_dir/assets`. This is the
    // whole `dist/` tree, because request paths already carry the
    // `/assets/` prefix (`<script src="/assets/index-<hash>.js">`).
    // Rooting at `spa_dir/assets` made ServeDir look for
    // `dist/assets/assets/index-<hash>.js`, miss, and fall through to the
    // `index.html` fallback — so every JS/CSS request returned 200 with
    // `text/html`, the SPA never booted, and the app was blank. Status-code
    // assertions cannot see this; the body content-type is the tell.
    let spa_clone = spa_dir.clone();
    let serve_dir = ServeDir::new(spa_dir.clone()).fallback(
        tower_http::services::fs::ServeFile::new(spa_dir.join("index.html")),
    );

    // Pull base_path before the auth layer consumes `state` (the layer
    // takes ownership of a clone of `state.password`).
    let auth_state = crate::auth::AuthState::new(password.as_deref());

    // The API + MCP surface, behind the bearer middleware.
    //
    // Only this router is layered. The SPA shell and the static tree are
    // mounted *outside* it, because a browser cannot attach an
    // `Authorization: Bearer` header to a `<script type="module">` or a
    // stylesheet request. Gating `/assets/*` behind auth meant a
    // password-protected `serve-web` — which the README calls mandatory —
    // returned the shell at `/` but 401 for every JS/CSS file, so the app
    // rendered blank in the exact deployment the docs push.
    let guarded = Router::new()
        .merge(api)
        .merge(mcp_routes)
        .route("/", get(move || spa_index(spa_clone)))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            crate::auth::auth_middleware,
        ));

    // Everything not matched above is static: `/assets/*` (hashed JS/CSS),
    // `/favicon.svg`, and any client-side route, which falls back to the
    // shell. Public by construction, same as the shell at `/`.
    //
    // The fallback lives on the outer router because only one router in a
    // merge chain may own a fallback.
    let inner = Router::new().merge(guarded).fallback_service(serve_dir);

    // Optionally nest under `base_path` so a reverse proxy (nginx,
    // Caddy, Cloudflare) can host Simple Post at e.g.
    // `https://server/sp/`. When set, the entire router (including
    // the SPA fallback) is mounted under the prefix and `MatchedPath`
    // inside handlers reflects the strip-prefix path. When unset the
    // router is served at root.
    match base_path.as_deref() {
        Some(p) if !p.is_empty() => {
            eprintln!("[serve-web] base_path: {p}");
            // axum's `nest` matches the *bare* prefix (`/sp`) but never
            // forwards the trailing-slash spelling (`/sp/`) to the inner
            // router's `/` route — that request 404s. The shipped reverse
            // proxy examples redirect `/sp` → `/sp/`, so the documented
            // deployment landed users on a 404 at the app root.
            //
            // Serve the SPA shell for the trailing-slash form as well.
            // Deliberately no auth layer: this is the same public static
            // asset that `/sp` already serves (see `auth::is_public_path`,
            // which lists `/`), and the API underneath stays protected.
            let prefix = p.trim_end_matches('/');
            let spa_shell = spa_dir.clone();
            Router::new()
                .nest(prefix, inner)
                // The SPA shell references its bundle with **absolute**
                // URLs (`<script src="/assets/index-<hash>.js">`), so
                // nesting the router under `/sp` alone is not enough: the
                // browser would ask for `/assets/...`, which the nested
                // router never sees. Re-publish the static tree at the bare
                // `/assets` prefix so the documented deployment works
                // without requiring a hand-written rewrite in nginx.
                //
                // `nest_service` strips `/assets` before handing the
                // request to ServeDir, so it resolves
                // `dist/assets/<file>` — the same files `/sp/assets/*`
                // serves through the nested fallback.
                .nest_service("/assets", ServeDir::new(spa_dir.join("assets")))
                .route(&format!("{prefix}/"), get(move || spa_index(spa_shell)))
        }
        _ => inner,
    }
}

/// Dispatch a JSON-RPC request to the MCP dispatcher. Wraps
/// `daemon_http::dispatch_mcp_request` (the core of the daemon's
/// `/mcp` handler) so the `serve-web` shape can serve AI clients on
/// the same port without spawning a daemon child.
async fn mcp_json_rpc_post(state: Arc<McpAppState>, body: String) -> axum::response::Response {
    daemon_http::dispatch_mcp_request(state, body).await
}

async fn mcp_healthz(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
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
    password: Option<String>,
    base_path: Option<String>,
) -> Result<(), AppError> {
    let pool = db::init(&data_dir)?;
    let daemon = McpDaemonHandle::new();

    if auto_start_daemon {
        // Read the persisted `mcp.auto_start` row. The CLI flag can
        // override the persisted value, but defaults to honouring it.
        let want = db::with_conn(&pool, db::preferences::mcp_auto_start)?;
        if want {
            if let Err(e) = daemon.restart_without_app_handle(&data_dir).await {
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
        password,
        base_path,
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

// =========================================================================
// Public HTTP API contract (commit 2: chore(docs): HTTP API 公开契约)
// =========================================================================
//
// `RouteSpec` is the single source of truth for which `/api/*` routes
// are part of the stable contract. `scripts/dump-openapi.mjs` reads this
// list (plus the corresponding `WEB_ROUTES` entries in
// `src/lib/tauri-commands.ts`) to generate `docs/openapi.yaml`. Anything
// NOT in `public_route_specs()` is treated as `internal` — not part of
// the stable v1 contract, no backward-compat promise.
//
// Stability rules:
//   * `public`   — endpoint shape, auth, and response codes are stable.
//                  Breaking changes require a `/api/v2/*` migration.
//   * `internal` — no version commitment; UI / supervisor use only.
//
// This list is hand-curated and intentionally does NOT enumerate every
// axum router binding (e.g. `/mcp/*` from `daemon_http` are out of
// scope — they're a JSON-RPC MCP contract, documented separately).

#[derive(Debug, Clone, Serialize)]
pub struct RouteSpec {
    /// HTTP method (GET / POST / PUT / PATCH / DELETE).
    pub method: &'static str,
    /// Path template (axum syntax: `/api/projects/:id`).
    pub path: &'static str,
    /// Short description for the OpenAPI doc.
    pub summary: &'static str,
    /// `'public'` part of the stable v1 contract, or `'internal'`.
    pub stability: &'static str,
    /// Optional request body shape name (looked up by the OpenAPI dumper).
    /// `None` means "no body" or "shape not yet documented".
    pub request: Option<&'static str>,
    /// Optional response shape name. `None` means "shape not yet
    /// documented" — the dumper emits an `unknown` placeholder.
    pub response: Option<&'static str>,
}

/// Single source of truth for the public HTTP API. **Hand-curated** —
/// any new endpoint that should be in the v1 stable contract MUST be
/// added here. Anything absent is `internal` and not part of OpenAPI.
pub fn public_route_specs() -> Vec<RouteSpec> {
    vec![
        // ---- health -----------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/healthz",
            summary: "Liveness probe (no auth)",
            stability: "public",
            request: None,
            response: Some("HealthzResponse"),
        },
        // ---- projects ---------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/projects",
            summary: "List projects",
            stability: "public",
            request: None,
            response: Some("Project[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects",
            summary: "Create a project",
            stability: "public",
            request: Some("CreateProject"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/projects/:id",
            summary: "Get one project",
            stability: "public",
            request: None,
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/projects/:id",
            summary: "Patch a project",
            stability: "public",
            request: Some("ProjectPatch"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/projects/:id",
            summary: "Delete a project",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        // ---- modules ----------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/modules",
            summary: "List modules in a project",
            stability: "public",
            request: None,
            response: Some("Module[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/modules",
            summary: "Create a module",
            stability: "public",
            request: Some("CreateModule"),
            response: Some("Module"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/modules/:id",
            summary: "Patch a module",
            stability: "public",
            request: Some("ModulePatch"),
            response: Some("Module"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/modules/:id",
            summary: "Delete a module",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/modules/move",
            summary: "Move a module within a project",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Module"),
        },
        // ---- collections ------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/collections",
            summary: "List collections",
            stability: "public",
            request: None,
            response: Some("Collection[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/collections",
            summary: "Create a collection",
            stability: "public",
            request: Some("CreateCollection"),
            response: Some("Collection"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/collections/:id",
            summary: "Patch a collection",
            stability: "public",
            request: Some("CollectionPatch"),
            response: Some("Collection"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/collections/:id",
            summary: "Delete a collection",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/collections/move",
            summary: "Move a collection",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Collection"),
        },
        // ---- endpoints --------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/endpoints",
            summary: "List endpoints",
            stability: "public",
            request: None,
            response: Some("Endpoint[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints",
            summary: "Create an endpoint",
            stability: "public",
            request: Some("CreateEndpoint"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/endpoints/:id",
            summary: "Get one endpoint",
            stability: "public",
            request: None,
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/endpoints/:id",
            summary: "Patch an endpoint",
            stability: "public",
            request: Some("EndpointPatch"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/endpoints/:id",
            summary: "Delete an endpoint",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/search",
            summary: "Search endpoints by name/method/url",
            stability: "public",
            request: Some("SearchQuery"),
            response: Some("Endpoint[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/move",
            summary: "Move an endpoint",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/:id/duplicate",
            summary: "Duplicate an endpoint",
            stability: "public",
            request: None,
            response: Some("Endpoint"),
        },
        // ---- baseUrl definitions ----------------------------------------
        // All five were live but undocumented. They are project-scoped
        // for reads / creates / set-default and id-scoped for mutations,
        // matching the two route families the handlers comment describes.
        RouteSpec {
            method: "GET",
            path: "/api/projects/:projectId/baseurls",
            summary: "List a project's base URL definitions",
            stability: "public",
            request: None,
            response: Some("BaseUrlDefinition[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls",
            summary: "Create a base URL definition in a project",
            stability: "public",
            request: Some("BaseUrlDefinition"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls/default",
            summary: "Set (or clear) the project's default base URL definition",
            stability: "public",
            request: Some("BaseUrlDefSetDefault"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/baseurl-defs/:defId",
            summary: "Rename or re-describe a base URL definition",
            stability: "public",
            request: Some("BaseUrlDefPatch"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/baseurl-defs/:defId",
            summary: "Delete a base URL definition",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        // ---- environments -----------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/environments",
            summary: "List environments in a project",
            stability: "public",
            request: None,
            response: Some("Environment[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments",
            summary: "Create an environment",
            stability: "public",
            request: Some("CreateEnvironment"),
            response: Some("Environment"),
        },
        // ---- http send --------------------------------------------------
        RouteSpec {
            method: "POST",
            path: "/api/http/send",
            summary: "Execute an HTTP request (bypasses webview CORS)",
            stability: "public",
            request: Some("HttpRequestInput"),
            response: Some("HttpResult"),
        },
        // ---- internal (not in OpenAPI v1) -------------------------------
        // The following endpoints are intentionally NOT listed above —
        // they are supervisor / UI projections and have no version
        // commitment. They appear in `WEB_ROUTES` for runtime access but
        // the OpenAPI v1 doc omits them.
        //   GET  /api/mcp/status             - daemon status (UI)
        //   POST /api/mcp/start              - daemon launch
        //   POST /api/mcp/stop               - daemon stop
        //   POST /api/mcp/restart            - daemon restart
        //   GET  /api/mcp/proxy-path         - mcp_proxy binary path
        //   GET  /api/mcp/capability         - capability policy (commit 1)
        //   PUT  /api/mcp/capability         - capability policy (commit 1)
        //   GET  /api/mcp/capability/tools   - visible tools under current policy
        //   GET  /api/preferences            - raw preferences dump (UI)
        //   PUT  /api/preferences            - raw preferences patch
        // /api/permissions* and /api/request_settings* are intentionally
        // absent from the public contract. The baseUrl-def family is now
        // documented above.
    ]
}
