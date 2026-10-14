//! Headless server host for Simple Post.
//!
//! This crate is everything the desktop shell and the container image
//! share, minus the GUI. It owns:
//!
//!   * `web_api` — the axum router behind `serve-web`: the REST surface,
//!     the `/mcp` JSON-RPC endpoint, and the static SPA host.
//!   * `daemon_supervisor` — the host-agnostic MCP daemon spawn /
//!     teardown logic.
//!   * `auth` — the bearer middleware and its public-path allowlist.
//!   * `port_registry` — the `mcp.port` file the stdio bridge discovers.
//!   * `dto` / `http_config` — wire shapes and daemon-config helpers
//!     shared by both transports.
//!
//! ## Why this is a separate crate
//!
//! `src-tauri` links `tauri` unconditionally, and Tauri pulls the whole
//! GTK/WebKit stack on Linux. That made "run the server" impossible to
//! containerise: the image needed X11 libraries for a GUI it can never
//! display, and the `--lib` test binary dragged the Windows GUI runtime
//! in through an `AppHandle` (see the long note on `daemon_supervisor`).
//! Splitting the server out means the image is `debian-slim` plus one
//! binary, and the test binary links nothing it has no business linking.
//!
//! The dependency direction is one-way:
//!
//! ```text
//! simple-post (Tauri)  ->  simple-post-server  ->  simple-post-core
//!                                     |
//!                                     +-------------->  simple-post-mcp
//! ```
//!
//! `src-tauri` re-exports the modules below so the existing
//! `crate::daemon_supervisor::…` / `crate::web_api::…` paths in the
//! Tauri commands keep resolving.

pub mod auth;
pub mod daemon_supervisor;
pub mod dto;
pub mod http_config;
pub mod port_registry;
pub mod web_api;

// Re-export the shared low-level layer so call sites can keep using
// `crate::db::*` / `crate::domain::*` / `crate::error::*` /
// `crate::permissions::*` / `crate::http_executor::*` /
// `crate::mcp_metrics::*` / `crate::mcp::*` after the workspace
// extraction and this crate split.
pub use simple_post_core::{db, domain, error, http_executor, mcp_metrics, permissions};

// Re-export the MCP module from the standalone `simple-post-mcp` crate.
// `daemon_http` is re-exported because `run_mcp` needs `serve_with_config`
// to hand the listener configuration (mount path + Host/Origin allowlists)
// to the router, which `mcp::serve_http` has no parameter for.
pub use simple_post_mcp::{daemon_http, mcp};

/// CLI subcommand `mcp` entry. Owns data-dir resolution, DB init, axum serve,
/// port-record write, and SIGINT/SIGTERM shutdown.
///
/// `http_config` carries the listener settings the router needs beyond
/// `bind` / `port`: the mount path and the Host/Origin allowlists. The
/// supervisor fills it from the `mcp.http_config` preference
/// (`daemon_cli_args`); a hand-run `simple-post mcp` gets clap's
/// defaults, which are the loopback-only shipped configuration.
pub fn run_mcp(
    transport: String,
    port: u16,
    bind: String,
    http_config: crate::domain::McpHttpConfig,
) {
    init_tracing();

    // A hand-run daemon can still be pointed at a contradictory
    // configuration (`--bind=0.0.0.0` without `--allow-remote`), and
    // failing here is far better than serving a LAN port that the Host
    // guard answers with 403.
    if let Err(e) = http_config.validate() {
        eprintln!("[mcp] invalid listener configuration: {e}");
        std::process::exit(2);
    }

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
    let mount_path = http_config.normalized_path();
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

        // Honor `--transport=stdio` as well as the default `--transport=http`,
        // but route everything through the same axum router so we only have one
        // set of handlers to maintain. (The proxy binary is the recommended
        // stdio path; this fallback is mostly here so the CLI argument
        // exists and can be added back if AI clients ever support direct
        // HTTP MCP connections.)
        let _ = transport; // accepted for forward-compat; not yet branched on.

        // Bind + spawn here rather than through `mcp::serve_http`: that
        // helper predates the listener configuration and has no way to
        // pass the mount path / allowlists into `daemon_http::serve`.
        // The block below is the same bind → resolve port → spawn
        // sequence, with `serve_with_config` in place of `serve`.
        let listener = match tokio::net::TcpListener::bind((bind_for_log.as_str(), port)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[mcp] bind {bind_for_log}:{port} failed: {e}");
                std::process::exit(1);
            }
        };
        let actual_port = match listener.local_addr() {
            Ok(a) => a.port(),
            Err(e) => {
                eprintln!("[mcp] local_addr failed: {e}");
                std::process::exit(1);
            }
        };

        let state = std::sync::Arc::new(crate::mcp::AppState {
            pool,
            started_at: std::time::Instant::now(),
            metrics: std::sync::Arc::new(crate::mcp_metrics::McpMetrics::new()),
        });
        tokio::spawn(async move {
            if let Err(e) =
                crate::daemon_http::serve_with_config(listener, state, http_config).await
            {
                tracing::error!(error = %e, "MCP daemon exited with error");
            }
        });

        eprintln!(
            "[mcp] daemon listening on http://{bind_for_log}:{actual_port}{mount_path} \
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
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
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
         (or connect the client to the daemon over HTTP)."
    );
    std::process::exit(2);
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
/// only via the REST bridge; AI clients connect either to the
/// `simple-post-mcp` binary on their own machine, or straight to this
/// host's HTTP endpoint.
///
pub async fn run_serve_web(
    bind_addr: std::net::SocketAddr,
    spa_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    auto_start_daemon: Option<bool>,
    password: Option<String>,
    base_path: Option<String>,
) {
    init_tracing();
    if let Err(e) = web_api::serve(
        bind_addr,
        data_dir,
        spa_dir,
        auto_start_daemon,
        password,
        base_path,
    )
    .await
    {
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
