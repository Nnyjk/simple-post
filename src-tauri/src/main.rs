// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, Subcommand};

/// Command-line entry point for `simple-post`.
///
/// Subcommands:
///   - (no subcommand) → launch the Tauri GUI
///   - `mcp`           → run as MCP daemon (HTTP transport by default)
///   - `mcp-proxy`     → run as stdio↔HTTP proxy (separate binary; refused)
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
    /// (`simple-post-mcp-proxy.exe`); the main binary prints an error and exits.
    #[command(name = "mcp-proxy")]
    McpProxy,
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
            eprintln!("mcp-proxy is a separate binary: simple-post-mcp-proxy.exe");
            std::process::exit(2);
        }

        // No subcommand → launch the Tauri GUI.
        None => simple_post_lib::run(),
    }
}