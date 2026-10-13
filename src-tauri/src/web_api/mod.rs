//! REST + static-file HTTP layer for the `simple-post serve-web` shape.
//!
//! Browser users hit this on the LAN to read / write the same SQLite
//! data the Tauri shell reads. The daemon JSON-RPC entry (`/mcp`)
//! stays untouched so MCP clients (Claude Desktop, Cursor, …) keep
//! working unchanged.
//!
//! Layout: `WebState` + `build_router` + the `serve` entry point live
//! here; handlers are split one module per resource under `routes/`,
//! wire types in `dto.rs`, the `AppError` → HTTP mapping in `error.rs`,
//! the OpenAPI contract table in `spec.rs`, and the SPA static
//! handlers in `static_files.rs`.

mod dto;
mod error;
mod routes;
mod spec;
mod static_files;

pub use dto::{ensure_server_id, HealthDto, McpStatusDto};
pub use error::{map_app_error, HttpErrorResponse};
pub use spec::{public_route_specs, RouteSpec};

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    http::Method,
    routing::{get, patch, post},
    Json, Router,
};
use tower_http::services::ServeDir;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::mcp::AppState as McpAppState;
use crate::port_registry;

use routes::*;
use static_files::spa_index;

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
    /// unauthenticated (the backwards-compatible default).
    /// `Some(_)` enforces the `auth_middleware` on every `/api/*`
    /// and `/mcp` request. The frontend caches the bearer in
    /// `localStorage` and attaches it via `webFetch()`.
    pub password: Option<String>,
    /// Optional reverse-proxy base path (e.g. `"/sp"` for an
    /// nginx-prefixed deployment). When set, the entire router is
    /// `nest`ed under this prefix; `curl /api/projects` returns 404
    /// and `curl /sp/api/projects` returns 200.
    pub base_path: Option<String>,
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
/// default.
pub fn build_router(state: WebState, spa_dir: PathBuf) -> Router {
    // Pull the auth fields before `Arc::new` consumes the struct so the
    // `auth_middleware` can read them without holding the heavy state.
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
        // ---- resource tree ----
        //
        // Replaces the 19 `/api/modules/*`, `/api/collections/*` and
        // `/api/endpoints/*` paths these used to be spread across. The
        // literal action verbs are registered before `/:id` so they win
        // in the router's radix tree.
        .route("/api/nodes", get(nodes_list).post(nodes_create))
        .route("/api/nodes/move", post(nodes_move))
        .route(
            "/api/nodes/:id",
            get(nodes_get).patch(nodes_update).delete(nodes_delete),
        )
        .route(
            "/api/nodes/:id/endpoint-detail",
            get(node_endpoint_detail_get).patch(node_endpoint_detail_patch),
        )
        // The bulk read the renderer's bootstrap issues. Registered on
        // its own path rather than as a filter on `/api/nodes` because it
        // answers a different question — configuration, not structure.
        .route("/api/endpoint-details", get(endpoint_details_list))
        .route("/api/nodes/:id/duplicate", post(node_duplicate))
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
            "/api/environments/:envId/vars/rename",
            post(environments_rename_var),
        )
        .route(
            "/api/environments/:envId/module-baseurls",
            post(environments_set_module_baseurl),
        )
        // ---- baseUrlDefs ----
        // A service belongs to the project, so all three project-scoped
        // verbs hang off `/api/projects/:projectId/baseurls`.
        // `/baseurls/default` is a literal sibling of the
        // `/api/baseurl-defs/:defId` parametrised route's parent — it is
        // a different prefix entirely, so no radix-tree precedence
        // question arises; `base_url_defs_test` in this crate pins that
        // every one of them stays reachable.
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
        // ---- mcp capability policy ----
        .route(
            "/api/mcp/capability",
            get(mcp_capability_get).put(mcp_capability_put),
        )
        .route("/api/mcp/capability/tools", get(mcp_capability_list_tools))
        // ---- mcp http listener config (daemon bind/path/allowlists) ----
        .route(
            "/api/mcp/http-config",
            get(mcp_http_config_get).put(mcp_http_config_put),
        )
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

// -------------------------------------------------------------------------
// Entry point used by `lib.rs::run_serve_web`.
// -------------------------------------------------------------------------

/// Bind a TCP listener and serve `router` until SIGINT/SIGTERM.
///
/// `data_dir` is used by the daemon supervisor to resolve the SQLite
/// file; it must match the GUI's `dirs::data_dir().join("com.simple-post")`
/// convention so the two shapes share the same database.
///
/// `auto_start_daemon` is `Some(bool)` when the CLI forced the answer and
/// `None` when the persisted `mcp.auto_start` row decides. Resolving that
/// row here rather than in `main.rs` is deliberate: `main.rs` used to open
/// its own `db::init()` pool purely to read one preference and then throw it
/// away, so every cold web start paid for two full initialisations (each one
/// a schema check, the whole `schema.sql`, and a 4-connection pool).
pub async fn serve(
    bind_addr: std::net::SocketAddr,
    data_dir: PathBuf,
    spa_dir: PathBuf,
    auto_start_daemon: Option<bool>,
    password: Option<String>,
    base_path: Option<String>,
) -> Result<(), AppError> {
    let pool = db::init(&data_dir)?;
    // The supervisor reads `mcp.http_config` from this pool at spawn time
    // so the daemon child's CLI matches the settings panel.
    let daemon = McpDaemonHandle::with_pool(pool.clone());

    // Resolve the daemon decision *before* either value is moved into
    // `WebState`, so no second `db::init()` is needed and the handle the
    // background task warms up is the same one `/api/mcp/*` reads and writes.
    let want_daemon = match auto_start_daemon {
        Some(v) => v,
        None => db::with_conn(&pool, db::preferences::mcp_auto_start).unwrap_or(false),
    };

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
        daemon: daemon.clone(),
        data_dir: data_dir.clone(),
        mcp_state,
        password,
        base_path,
    };
    let router = build_router(state, spa_dir);

    eprintln!("[serve-web] listening on http://{bind_addr}");
    eprintln!("[serve-web] data dir: {}", data_dir.display());

    // -----------------------------------------------------------------------
    // Bind BEFORE warming the MCP daemon.
    //
    // This ordering is the whole point. `restart_without_app_handle` costs a
    // fixed 200 ms sleep, two `tasklist` spawns on Windows (it probes PID
    // liveness twice), a full spawn of this same executable in `mcp` mode,
    // and a healthz probe — and it used to run *before* `TcpListener::bind`.
    // The browser sat on a refused connection for all of it, which reads as
    // a blank page. Measured locally: first byte at ~344 ms with
    // auto-start on vs ~90 ms with it off.
    //
    // Binding first turns that into a genuine background task: the daemon
    // comes up in parallel with the page loading, and `/api/mcp/status`
    // reports `Starting` until it is ready — the same transient the GUI
    // already surfaces.
    // -----------------------------------------------------------------------
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Validation(format!("bind {bind_addr}: {e}")))?;

    if want_daemon {
        let dir = data_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = daemon.restart_without_app_handle(&dir).await {
                eprintln!("[serve-web] ensure_started failed: {e:?}");
            }
        });
    } else {
        eprintln!(
            "[serve-web] mcp.auto_start is off — daemon left stopped; \
             use POST /api/mcp/start or the settings panel to enable."
        );
    }

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
