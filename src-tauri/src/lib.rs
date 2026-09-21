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
//!   - Boot the embedded MCP HTTP daemon as a child process
//!     (`daemon_supervisor::McpDaemonHandle`), so the React UI can
//!     surface live daemon status via the `mcp_status` command.
//!   - Dispatch the `mcp` CLI subcommand (used when the binary is
//!     launched directly as a daemon) through `run_mcp`, and route the
//!     `mcp-proxy` subcommand to a hard refusal so users get a clean
//!     error pointing them at the dedicated proxy binary.

use tauri::Manager;

pub mod commands;
pub mod daemon_supervisor;
pub mod port_registry;
pub mod web_api;

// Re-export the shared low-level layer so call sites can keep using
// `crate::db::*` / `crate::domain::*` / `crate::error::*` /
// `crate::permissions::*` / `crate::http_executor::*` /
// `crate::mcp_metrics::*` after the workspace extraction (commit 3).
pub use simple_post_core::{
    db, domain, error, http_executor, mcp_metrics, permissions,
};

// Re-export the MCP module from the standalone `simple-post-mcp` crate.
// The main crate only hosts the embedded daemon + `serve-web` glue;
// the actual JSON-RPC dispatcher lives there.
pub use simple_post_mcp::mcp;

/// CLI subcommand `mcp` entry. Owns data-dir resolution, DB init, axum serve,
/// port-record write, and SIGINT/SIGTERM shutdown.
pub fn run_mcp(transport: String, port: u16, bind: String) {
    init_tracing();

    // Resolve data dir *the same way* `port_registry` does so the SQLite
    // file the daemon reads/writes is alongside the `mcp.port` file
    // the proxy discovers. Both honor `SIMPLE_POST_DATA_DIR` together.
    let data_dir = match crate::port_registry::data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[mcp] cannot resolve data dir: {e:?}");
            std::process::exit(1);
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[mcp] failed to create tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    let bind_for_log = bind.clone();
    runtime.block_on(async move {
        // Open the SQLite pool first so the daemon's tools can read/write
        // immediately on first request — no first-call warm-up cost.
        let pool = match crate::db::init(&data_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[mcp] db::init({}) failed: {e:?}", data_dir.display());
                std::process::exit(1);
            }
        };

        // Honor `--stdio` for parity with the `--http` documented form, but
        // route everything through the same axum router so we only have one
        // set of handlers to maintain. (The proxy binary is the recommended
        // stdio path; this fallback is mostly here so the CLI argument
        // exists and can be added back if AI clients ever support direct
        // HTTP MCP connections.)
        let _ = transport; // accepted for forward-compat; not yet branched on.

        let actual_port = match crate::mcp::serve_http(pool, port, &bind).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[mcp] serve_http({bind_for_log}:{port}) failed: {e:?}");
                std::process::exit(1);
            }
        };
        eprintln!(
            "[mcp] daemon listening on http://{bind_for_log}:{actual_port}/mcp \
             (data dir: {})",
            data_dir.display()
        );

        // Publish the live port so `simple-post-mcp` and any
        // other AI client can discover us. The CLI mode is the
        // canonical way to run the daemon for tests and headless
        // automation; without this write the proxy would refuse to
        // start ("daemon not running").
        let port_record = crate::port_registry::PortRecord {
            pid: std::process::id(),
            port: actual_port,
            bind: bind_for_log.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            data_dir: data_dir.display().to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        if let Err(e) = crate::port_registry::write(&port_record) {
            let path_label = crate::port_registry::port_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unresolved>".to_string());
            eprintln!("[mcp] WARN: failed to write {path_label}: {e:?}");
        }

        // Block until SIGINT/SIGTERM. `tokio::signal` returns the
        // Ctrl-C/SIGINT future and a separate SIGTERM handler for
        // non-Windows; on Windows only Ctrl-C is meaningfully delivered.
        let shutdown = async {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install SIGTERM handler");
                tokio::select! {
                    _ = ctrl_c => {},
                    _ = sigterm.recv() => {},
                }
            }
            #[cfg(not(unix))]
            {
                let _ = ctrl_c.await;
            }
        };
        shutdown.await;
        eprintln!("[mcp] shutdown signal received, exiting");
        // Best-effort: drop the port file so the next `simple-post.exe mcp`
        // (or the GUI supervisor) doesn't see a stale record.
        let _ = crate::port_registry::release();
    });
}

/// CLI subcommand `mcp-proxy` entry. The proxy is a separate binary
/// (`simple-post-mcp[.exe]`); running it via the main binary is
/// almost certainly a user mistake (wrong PATH, wrong shortcut), so
/// point them at the right binary and exit non-zero so a CI script
/// would catch it.
pub fn run_mcp_proxy() {
    eprintln!(
        "mcp-proxy is a separate binary — please run `simple-post-mcp` instead \
         (or `npx -y @simple-post/mcp-server`)."
    );
    std::process::exit(2);
}

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

    let daemon_handle = McpDaemonHandle::new();

    tauri::Builder::default()
        .manage(pool)
        .manage(daemon_handle)
        .invoke_handler(tauri::generate_handler![
            // Local HTTP request command (kept here from pre-MCP days).
            http_request,
            crate::commands::projects_list,
            crate::commands::projects_get,
            crate::commands::projects_create,
            crate::commands::projects_update,
            crate::commands::projects_delete,
            crate::commands::modules_list,
            crate::commands::modules_create,
            crate::commands::modules_update,
            crate::commands::modules_delete,
            crate::commands::modules_move,
            crate::commands::collections_list,
            crate::commands::collections_create,
            crate::commands::collections_update,
            crate::commands::collections_delete,
            crate::commands::collections_move,
            crate::commands::endpoints_list,
            crate::commands::endpoints_get,
            crate::commands::endpoints_search,
            crate::commands::endpoints_create,
            crate::commands::endpoints_update,
            crate::commands::endpoints_delete,
            crate::commands::endpoints_duplicate,
            crate::commands::endpoints_move,
            crate::commands::environments_list,
            crate::commands::environments_create,
            crate::commands::environments_update,
            crate::commands::environments_delete,
            crate::commands::environments_set_var,
            crate::commands::environments_set_baseurl,
            crate::commands::environments_set_active,
            crate::commands::base_url_defs_list,
            crate::commands::base_url_defs_create,
            crate::commands::base_url_defs_update,
            crate::commands::base_url_defs_delete,
            crate::commands::base_url_defs_set_default,
            crate::commands::request_settings_get,
            crate::commands::request_settings_update,
            crate::commands::permissions_get,
            crate::commands::permissions_set,
            crate::commands::mcp_status,
            crate::commands::mcp_daemon_restart,
            crate::commands::mcp_daemon_proxy_path,
            crate::commands::mcp_auto_start_get,
            crate::commands::mcp_auto_start_set,
            crate::commands::mcp_capability_get,
            crate::commands::mcp_capability_set,
            crate::commands::mcp_capability_list_tools,
        ])
        .setup(|app| {
            // Read the persisted MCP auto-start preference **before**
            // spawning the supervisor so we honour the user's choice.
            // `mcp.auto_start` defaults to `false` — the user opts in
            // from the settings panel. See
            // `docs/distribution-modes-2026-09-18.md` §5.
            let pool: tauri::State<crate::db::DbPool> = app.state();
            let auto_start = match crate::db::with_conn(pool.inner(), |c| {
                crate::db::preferences::mcp_auto_start(c)
            }) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "[gui] failed to read mcp.auto_start, defaulting to off: {e:?}"
                    );
                    false
                }
            };

            // `app.state::<McpDaemonHandle>()` borrows the managed
            // value; we clone the (cheap, Arc-backed) handle and move
            // it into the spawn.
            let handle = app.state::<McpDaemonHandle>().inner().clone();
            let app_handle = app.handle().clone();
            tokio::spawn(async move {
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

/// CLI subcommand `serve-web` entry. Boots an axum HTTP server that:
///
///   * serves the bundled SPA from `<project>/dist/`,
///   * exposes `/api/*` REST routes for browser CRUD + HTTP send + prefs,
///   * keeps the existing `/mcp` JSON-RPC router untouched so MCP
///     clients (Claude Desktop, Cursor, …) keep working unchanged.
///
/// The browser never spawns a daemon — it talks to `127.0.0.1:<port>`
/// directly. The daemon child (if `mcp.auto_start == true`) runs on
/// this host, behind the `simple-post` process tree, and is reachable
/// only via the REST bridge; AI clients continue to use the
/// `simple-post-mcp` binary on their own machine (or
/// `npx -y @simple-post/mcp-server`) as before.
///
/// See `docs/distribution-modes-2026-09-18.md` §3 + §6 for the
/// architecture and route table.
pub async fn run_serve_web(
    bind_addr: std::net::SocketAddr,
    spa_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    auto_start_daemon: bool,
) {
    init_tracing();
    if let Err(e) = web_api::serve(bind_addr, data_dir, spa_dir, auto_start_daemon).await {
        eprintln!("[serve-web] fatal: {e:?}");
        std::process::exit(1);
    }
}

/// Initialize the global `tracing` subscriber so the `tracing::info!` /
/// `tracing::warn!` / `tracing::error!` macros across the crate actually
/// emit output.
///
/// Honors `RUST_LOG` (via `EnvFilter`); defaults to `info` if unset.
/// Uses `try_init` so a duplicate init (tests + GUI + CLI) silently no-ops
/// instead of panicking.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}