//! Tauri 2 backend for Simple Post.
//!
//! Responsibilities:
//!   - Expose the `http_request` command so the React app can issue
//!     real network calls through `reqwest` instead of the browser's
//!     `fetch`. The webview doesn't enforce CORS when the call exits
//!     through a Rust-side HTTP client, which is what fixes the
//!     "Network Error" reported when the app was launched as a plain
//!     Vite SPA.
//!   - Match the contract of `src/lib/http.ts::sendRequest` so the
//!     frontend can swap one transport for the other without changing
//!     call sites.
//!   - Wire the `#[tauri::command]` surface in `commands/` onto managed
//!     state, and boot the embedded MCP daemon as a child process so
//!     the UI can surface live daemon status.
//!
//! ## What moved out
// The server half of this crate — `web_api`, `daemon_supervisor`,
//! `auth`, `port_registry`, `dto`, `http_config`, and the `mcp` /
//! `serve-web` entry points — now lives in `simple-post-server`. It is
//! re-exported below so every existing `crate::…` path in `commands/`
//! keeps resolving and nothing above this line had to change.
//!
//! The reason is `tauri` itself: it is an unconditional dependency of
//! this crate, so a Linux container serving `serve-web` had to install
//! the whole GTK/WebKit stack to run a GUI it cannot display. The
//! dependency now points the other way — this crate depends on the
//! server crate — so the image can be built Tauri-free.

use tauri::Manager;

pub mod commands;

pub use simple_post_core::{db, domain, error, http_executor, mcp_metrics, permissions};
pub use simple_post_mcp::{daemon_http, mcp};

// The headless host, re-exported wholesale. Keeping these `pub use`
// lines rather than thin forwarding wrappers is what lets the code in
// `commands/` be untouched by the crate split.
pub use simple_post_server::{auth, daemon_supervisor, dto, http_config, port_registry, web_api};

// CLI entry points. `serve-web` and `mcp` used to be defined here; they
// are host-agnostic and now belong to the server crate, but the
// desktop binary keeps dispatching to them so `cargo run -- serve-web`
// (and therefore `npm run dev:split`) behaves exactly as before.
pub use simple_post_server::{init_tracing, run_mcp, run_mcp_proxy, run_serve_web};

// ----- HTTP request command ------------------------------------------------
//
// The actual I/O lives in `crate::http_executor::execute`. The Tauri
// command body is a thin pass-through so the `generate_handler!` macro
// can still find the `http_request` symbol in `lib.rs`.

/// Issue a real HTTP request through `reqwest`, bypassing browser CORS.
///
/// The frontend calls this when running inside Tauri (where
/// `window.__TAURI_INTERNALS__` is present). When the app is served
/// just by Vite (no Tauri shell), the frontend falls back to the
/// existing `fetch` path so dev iteration on UI doesn't require a
/// Rust rebuild every time.
///
/// NOTE: this function MUST stay non-`pub`. Tauri 2's command +
/// `generate_handler!` macros both emit an internal
/// `__cmd__http_request` macro; emitting them in the same crate-root
/// file with `pub` collides on the macro namespace (E0255). See
/// https://v2.tauri.app/develop/calling-rust/ — "Commands defined in
/// the lib.rs file cannot be marked as pub".
#[tauri::command]
async fn http_request(
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout_ms: u64,
    follow_redirects: bool,
    max_response_size_kb: u64,
) -> crate::http_executor::HttpResult {
    let input = crate::http_executor::HttpRequestInput {
        method,
        url,
        headers,
        body,
        timeout_ms,
        follow_redirects,
        max_response_size_kb,
    };
    crate::http_executor::execute(&input).await
}

// ----- Tauri entry point ----------------------------------------------------

/// Tauri GUI entry. Wires:
///   * `McpDaemonHandle` into managed state so the `mcp_status` command
///     reads it via `State<McpDaemonHandle>`.
///   * All DB / permissions commands into the invoke handler.
///   * A `.setup()` block that asynchronously spawns the daemon child
///     the moment the GUI finishes its own boot.
///
/// The daemon supervisor is launched in `tokio::spawn` rather than
/// awaited inline so the GUI window can render immediately and the
/// supervisor's 5-second healthz wait doesn't sit on the main run-loop.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    use crate::daemon_supervisor::McpDaemonHandle;

    // Open the GUI's own DB pool before launching the webview so every
    // `#[tauri::command]` that takes `State<DbPool>` resolves on the
    // very first invoke. Same data-dir convention as the daemon —
    // SQLite WAL keeps both processes consistent on the shared file.
    let data_dir = match crate::port_registry::data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[gui] cannot resolve data dir: {e:?}");
            std::process::exit(1);
        }
    };
    let pool = match crate::db::init(&data_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[gui] db::init({}) failed: {e:?}", data_dir.display());
            std::process::exit(1);
        }
    };

    let daemon_handle = McpDaemonHandle::with_pool(pool.clone());

    tauri::Builder::default()
        .manage(pool)
        .manage(daemon_handle)
        .invoke_handler(tauri::generate_handler![
            // Local HTTP request command (kept here from pre-MCP days).
            http_request,
            // Endpoint-scoped send is deliberately absent: the frontend
            // assembles the full request and hands it to `http_request`.
            crate::commands::projects_list,
            crate::commands::projects_get,
            crate::commands::projects_create,
            crate::commands::projects_update,
            crate::commands::projects_delete,
            crate::commands::nodes_list,
            crate::commands::nodes_get,
            crate::commands::nodes_create,
            crate::commands::nodes_update,
            crate::commands::nodes_delete,
            crate::commands::nodes_move,
            crate::commands::nodes_duplicate,
            crate::commands::endpoint_detail_get,
            crate::commands::endpoint_details_list,
            crate::commands::endpoint_detail_update,
            crate::commands::environments_list,
            crate::commands::environments_create,
            crate::commands::environments_update,
            crate::commands::environments_delete,
            crate::commands::environments_set_var,
            crate::commands::environments_rename_var,
            crate::commands::environments_set_module_baseurl,
            crate::commands::environments_set_active,
            crate::commands::base_url_defs_list,
            crate::commands::base_url_defs_create,
            crate::commands::base_url_defs_update,
            crate::commands::base_url_defs_delete,
            crate::commands::base_url_defs_set_default,
            crate::commands::request_settings_get,
            crate::commands::request_settings_update,
            // Response history (per-endpoint FIFO, capped in the db
            // write path). Registered as its own group because it is
            // the one resource whose reads are project-scoped rather
            // than endpoint-scoped.
            crate::commands::response_history_list,
            crate::commands::response_history_append,
            crate::commands::response_history_clear,
            crate::commands::permissions_get,
            crate::commands::permissions_set,
            crate::commands::mcp_status,
            crate::commands::mcp_daemon_start,
            crate::commands::mcp_daemon_stop,
            crate::commands::mcp_daemon_restart,
            crate::commands::mcp_daemon_proxy_path,
            crate::commands::mcp_auto_start_get,
            crate::commands::mcp_auto_start_set,
            crate::commands::mcp_capability_get,
            crate::commands::mcp_capability_set,
            crate::commands::mcp_capability_list_tools,
            crate::commands::mcp_http_config_get,
            crate::commands::mcp_http_config_set,
        ])
        .setup(|app| {
            // Read the persisted MCP auto-start preference **before**
            // spawning the supervisor so we honour the user's choice.
            // `mcp.auto_start` defaults to `false` — the user opts in
            // from the settings panel.
            let pool: tauri::State<crate::db::DbPool> = app.state();
            let auto_start = match crate::db::with_conn(pool.inner(), |c| {
                crate::db::preferences::mcp_auto_start(c)
            }) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[gui] failed to read mcp.auto_start, defaulting to off: {e:?}");
                    false
                }
            };

            // `app.state::<McpDaemonHandle>()` borrows the managed
            // value; we clone the (cheap, Arc-backed) handle and move
            // it into the spawn.
            let handle = app.state::<McpDaemonHandle>().inner().clone();
            let app_handle = app.handle().clone();
            // `tauri::async_runtime::spawn`, NOT `tokio::spawn` —
            // Tauri runs `.setup()` synchronously outside the tokio
            // runtime, so a raw `tokio::spawn` panics with
            // "there is no reactor running". Tauri's async_runtime
            // module has direct access to its tokio handle and spawns
            // tasks correctly from this context.
            tauri::async_runtime::spawn(async move {
                if !auto_start {
                    eprintln!(
                        "[gui] mcp.auto_start is off — leaving daemon stopped; \
                         toggle it in Settings → Connection to start."
                    );
                    return;
                }
                if let Err(e) = handle.ensure_started(&app_handle).await {
                    // Don't panic the GUI on daemon-spawn failure — the
                    // supervisor already flipped its status to Crashed
                    // and the renderer will show the message via
                    // `mcp_status` (or the toast from
                    // `daemon://crashed`).
                    eprintln!("[daemon] ensure_started failed: {e:?}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
