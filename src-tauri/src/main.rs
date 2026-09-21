// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, Subcommand};

/// Command-line entry point for `simple-post`.
///
/// Subcommands:
///   - (no subcommand) → launch the Tauri GUI
///   - `mcp`           → run as MCP daemon (HTTP transport by default)
///   - `mcp-proxy`     → run as stdio↔HTTP proxy (separate binary; refused)
///   - `serve-web`     → run the browser-facing REST + SPA host on a
///                       LAN-reachable port (the data lives on this
///                       host; the browser only sees the HTTP surface).
///                       See `docs/distribution-modes-2026-09-18.md` §3.
#[derive(Parser, Debug)]
#[command(name = "simple-post", version, about = "Modern desktop API tool with built-in MCP")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run as MCP daemon (HTTP transport by default).
    #[command(name = "mcp")]
    Mcp {
        /// Transport: `http` (default) or `stdio`. `stdio` here routes
        /// through the same HTTP server; use the separate
        /// `simple-post-mcp-proxy` binary for stdio mode.
        #[arg(long, default_value = "http")]
        transport: String,

        /// Port for HTTP transport. `0` means OS-assigned.
        #[arg(long, default_value_t = 0)]
        port: u16,

        /// Bind address. Use `0.0.0.0` to expose on the LAN.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

    /// Run as stdio→HTTP proxy. This is a separate binary
    /// (`simple-post-mcp.exe`); the main binary prints an error and exits.
    /// Use the dedicated binary directly or `npx -y @simple-post/mcp-server`.
    #[command(name = "mcp-proxy")]
    McpProxy,

    /// Run the browser-facing web server (REST + SPA). The browser
    /// never spawns an MCP daemon in this shape — all tool calls are
    /// delegated to this host. See `docs/distribution-modes-2026-09-18.md`.
    #[command(name = "serve-web")]
    ServeWeb {
        /// Bind address. `127.0.0.1` keeps the server local-only;
        /// `0.0.0.0` exposes the SPA + REST API on the LAN.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        /// Port to listen on.
        #[arg(long, default_value_t = 4173)]
        port: u16,

        /// Path to the built SPA bundle. Defaults to `<project_root>/dist`.
        #[arg(long)]
        spa_dir: Option<String>,

        /// Data directory. Defaults to the OS-conventional
        /// `dirs::data_dir()/simple-post` so the GUI and the server
        /// share the same SQLite file.
        #[arg(long)]
        data_dir: Option<String>,

        /// Override the persisted `mcp.auto_start` preference for this
        /// launch only. `--mcp-auto-start` forces the daemon to come up
        /// (and updates the row); `--no-mcp-auto-start` forces it to
        /// stay down. If neither flag is given, the persisted value is
        /// honoured.
        #[arg(long)]
        mcp_auto_start: Option<bool>,
    },
}

fn main() {
    simple_post_lib::init_tracing();

    let cli = Cli::parse();

    match cli.cmd {
        // MCP daemon — HTTP transport by default.
        Some(Cmd::Mcp {
            transport,
            port,
            bind,
        }) => {
            // Defer to the lib entry point so binary and lib share the same
            // daemon implementation.
            simple_post_lib::run_mcp(transport, port, bind);
        }

        // mcp-proxy is a separate binary; refuse to start here.
        Some(Cmd::McpProxy) => {
            eprintln!(
                "mcp-proxy is a separate binary:\n  \
                 - cargo-installed: cargo install simple-post-mcp\n  \
                 - npm: npx -y @simple-post/mcp-server"
            );
            std::process::exit(2);
        }

        // Browser-facing web host. Spawn a tiny tokio runtime, parse the
        // bind address, defer to the lib entry point.
        Some(Cmd::ServeWeb {
            bind,
            port,
            spa_dir,
            data_dir,
            mcp_auto_start,
        }) => {
            let bind_addr: std::net::SocketAddr = match format!("{bind}:{port}").parse() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("[serve-web] invalid --bind/--port: {e}");
                    std::process::exit(2);
                }
            };
            let spa_path = match spa_dir {
                Some(p) => std::path::PathBuf::from(p),
                None => simple_post_lib::web_api::default_spa_dir(),
            };
            let data_path = match data_dir {
                Some(p) => std::path::PathBuf::from(p),
                None => match simple_post_lib::web_api::default_data_dir() {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("[serve-web] cannot resolve data dir: {e:?}");
                        std::process::exit(2);
                    }
                },
            };

            // The CLI flag, when present, overrides the persisted row.
            let auto_start = match mcp_auto_start {
                Some(v) => {
                    // Persist the override so subsequent boots agree.
                    if let Ok(pool) = simple_post_lib::db::init(&data_path) {
                        let _ = simple_post_lib::db::with_tx(&pool, |c| {
                            simple_post_lib::db::preferences::set(
                                c,
                                simple_post_lib::db::preferences::keys::MCP_AUTO_START,
                                &v,
                            )
                        });
                    }
                    v
                }
                None => match simple_post_lib::db::init(&data_path) {
                    Ok(pool) => simple_post_lib::db::with_conn(&pool, |c| {
                        simple_post_lib::db::preferences::mcp_auto_start(c)
                    })
                    .unwrap_or(false),
                    Err(_) => false,
                },
            };

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("[serve-web] failed to build tokio runtime");
            runtime.block_on(simple_post_lib::run_serve_web(
                bind_addr,
                spa_path,
                data_path,
                auto_start,
            ));
        }

        // No subcommand → launch the Tauri GUI.
        None => simple_post_lib::run(),
    }
}