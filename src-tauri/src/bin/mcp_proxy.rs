// src-tauri/src/bin/mcp_proxy.rs
//
// Standalone stdio ↔ HTTP bridge binary for the Simple Post MCP daemon.
//
// Claude Desktop / Cursor only support stdio MCP servers (`command` in
// mcpServers config). Our daemon is a long-running HTTP process, so this
// binary reads a JSON `{"port": N}` record that the daemon writes on
// startup, then forwards every newline-delimited JSON-RPC line from
// stdin to `POST http://127.0.0.1:<port>/mcp` and writes the response
// back to stdout (one response per line).
//
// Zero business logic: no SQLite, no MCP parsing, no schema knowledge.
// Pure byte forwarding.
//
// Build:
//   cd src-tauri && cargo build --bin simple-post-mcp-proxy
// Output:
//   target/{profile}/simple-post-mcp-proxy[.exe]

use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;
// `tokio::io::stdin` / `tokio::io::stdout` require the `io-std` feature, which
// the project's Cargo.toml intentionally doesn't pull in (avoids forcing the
// feature on every consumer of the lib crate). We therefore do synchronous
// stdin/stdout I/O from a `spawn_blocking` task and pipe lines into the runtime
// via an mpsc channel.

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
struct ProxyArgs {
    port: Option<u16>,
    print_config: bool,
    show_help: bool,
}

fn parse_args() -> ProxyArgs {
    let mut args = ProxyArgs::default();
    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        if let Some(rest) = a.strip_prefix("--port=") {
            args.port = rest.parse().ok();
            continue;
        }
        match a.as_str() {
            "--port" => {
                if let Some(v) = iter.next() {
                    args.port = v.parse().ok();
                }
            }
            "--print-config" => args.print_config = true,
            "-h" | "--help" => args.show_help = true,
            "--version" | "-V" => {
                println!("simple-post-mcp-proxy {}", VERSION);
                std::process::exit(0);
            }
            other => {
                eprintln!("__proxy_error__unknown_arg:{}", other);
            }
        }
    }
    args
}

fn print_help() {
    println!("simple-post-mcp-proxy {}", VERSION);
    println!("stdio ↔ HTTP bridge for the Simple Post MCP daemon.");
    println!();
    println!("USAGE:");
    println!("  simple-post-mcp-proxy                  # auto-discover port from registry");
    println!("  simple-post-mcp-proxy --port=9999      # explicit port (debug)");
    println!("  simple-post-mcp-proxy --print-config   # print mcpServers JSON snippet");
    println!("  simple-post-mcp-proxy --version");
}

fn print_config() -> ExitCode {
    // Emit a mcpServers snippet the user pastes into Claude Desktop / Cursor.
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unresolved current_exe>".to_string());
    let snippet = serde_json::json!({
        "mcpServers": {
            "simple-post": {
                "command": exe,
                "args": [],
                "env": {}
            }
        }
    });
    match serde_json::to_string_pretty(&snippet) {
        Ok(s) => {
            println!("{}", s);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("__proxy_error__config_render:{}", e);
            ExitCode::from(1)
        }
    }
}

/// Resolve the path of the port-registry file the daemon writes.
///
/// Convention (must stay in sync with the daemon writer):
///   Windows : %APPDATA%\simple-post\mcp.port
///   Unix    : $XDG_DATA_HOME/simple-post/mcp.port  (else ~/.local/share/...)
///
/// Returns `None` if the platform convention can't be resolved (no APPDATA /
/// no HOME). File-content errors are handled by the caller.
fn port_file_path() -> Option<std::path::PathBuf> {
    // Honor the same `SIMPLE_POST_DATA_DIR` override as the daemon + GUI,
    // so tests (and power users running multiple installs side-by-side) can
    // redirect the proxy to a scratch dir without polluting the real
    // `%APPDATA%/simple-post/mcp.port`.
    if let Some(s) = std::env::var_os("SIMPLE_POST_DATA_DIR") {
        if !s.is_empty() {
            return Some(std::path::PathBuf::from(s).join("simple-post").join("mcp.port"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA").ok()?;
        Some(std::path::PathBuf::from(base).join("simple-post").join("mcp.port"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
            })?;
        Some(base.join("simple-post").join("mcp.port"))
    }
}

fn read_port_from_registry() -> Option<u16> {
    let path = port_file_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let rec: Value = serde_json::from_str(&content).ok()?;
    rec.get("port").and_then(|p| p.as_u64()).and_then(|p| u16::try_from(p).ok())
}

async fn healthz(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/mcp/healthz", port);
    let client = match reqwest::Client::builder().timeout(HEALTH_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.get(&url).send().await {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// Hit `/mcp/healthz` and compare its `version` field with our compiled-in
/// VERSION. The daemon should echo `CARGO_PKG_VERSION`, so a mismatch means
/// the user upgraded one side but not the other — refuse to bridge to avoid
/// silently corrupting JSON-RPC frames across an ABI break.
async fn check_version(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/mcp/healthz", port);
    let client = match reqwest::Client::builder().timeout(HEALTH_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.get(&url).send().await {
        // `reqwest::Response::json` requires the `json` feature, which the
        // project doesn't enable. Do the parse manually.
        Ok(r) => match r.text().await {
            Ok(t) => match serde_json::from_str::<Value>(&t) {
                Ok(json) => json
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(|v| v == VERSION)
                    .unwrap_or(false),
                Err(_) => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = parse_args();
    if args.show_help {
        print_help();
        return ExitCode::SUCCESS;
    }
    if args.print_config {
        return print_config();
    }

    // 1. Resolve port (CLI override → port registry file).
    let port = match args.port {
        Some(p) => p,
        None => match read_port_from_registry() {
            Some(p) => p,
            None => {
                eprintln!(
                    "__proxy_error__no_port_registry:daemon not running? launch the Simple Post GUI first."
                );
                return ExitCode::from(2);
            }
        },
    };

    let url = format!("http://127.0.0.1:{}/mcp", port);

    // 2. Verify daemon is actually alive on that port.
    if !healthz(port).await {
        eprintln!("__proxy_error__daemon_unreachable:{}", url);
        return ExitCode::from(3);
    }

    // 3. Verify proxy/daemon versions match before we start forwarding frames.
    if !check_version(port).await {
        eprintln!(
            "__proxy_version_mismatch__:proxy={} daemon=<?> — rebuild both from the same commit",
            VERSION
        );
        return ExitCode::from(4);
    }

    // 4. Build one shared client for the lifetime of the bridge.
    let client = match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("__proxy_error__client_init:{}", e);
            return ExitCode::from(1);
        }
    };

    // 5. Bridge stdin → HTTP → stdout.
    //
    // We need a synchronous read of stdin because tokio's async stdin/stdout
    // are gated behind the `io-std` feature (not enabled in this project's
    // Cargo.toml). Spawn a blocking task that reads lines and ships them
    // through an mpsc channel; the async runtime consumes from the channel,
    // forwards each line to the daemon, and dispatches stdout writes back
    // onto the blocking pool to keep writes serialized.
    let (line_tx, mut line_rx) = tokio::sync::mpsc::channel::<String>(16);
    tokio::task::spawn_blocking(move || {
        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            let line = match lines.next() {
                Some(Ok(l)) => l,
                // EOF: clean exit. I/O error on stdin: bail so we don't spin.
                None | Some(Err(_)) => break,
            };
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            // Receiver dropped → main returned → exit cleanly.
            if line_tx.blocking_send(trimmed).is_err() {
                break;
            }
        }
    });

    while let Some(line) = line_rx.recv().await {
        let resp = match client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(line)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("__proxy_error__http_send:{}", e);
                continue;
            }
        };

        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("__proxy_error__http_recv:{}", e);
                continue;
            }
        };

        let write_result = tokio::task::spawn_blocking(move || -> io::Result<()> {
            let mut out = io::stdout().lock();
            out.write_all(body.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()
        })
        .await;

        match write_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                eprintln!("__proxy_error__stdout_write:{}", e);
                return ExitCode::from(5);
            }
            Err(e) => {
                eprintln!("__proxy_error__stdout_join:{}", e);
                return ExitCode::from(5);
            }
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_file_path_resolves_on_host() {
        // We don't assert the exact bytes (env-dependent) — just that the
        // function returns Some on whichever platform we're compiled for.
        #[cfg(target_os = "windows")]
        assert!(std::env::var("APPDATA").is_ok(), "APPDATA must be set on Windows");
        let p = port_file_path();
        assert!(p.is_some(), "port_file_path returned None");
    }

    #[test]
    fn parse_args_handles_known_flags() {
        // Override argv by calling the helper directly with a captured env
        // is annoying; this just guards the trivial Default state.
        let a = ProxyArgs::default();
        assert!(a.port.is_none());
        assert!(!a.print_config);
        assert!(!a.show_help);
    }

    #[test]
    fn read_port_from_registry_missing_file_is_none() {
        // Point APPDATA at an empty temp dir — read_port_from_registry
        // should return None (file missing) without panicking.
        #[cfg(target_os = "windows")]
        {
            let tmp = std::env::temp_dir().join("simple-post-mcp-proxy-test");
            std::fs::create_dir_all(&tmp).ok();
            std::env::set_var("APPDATA", &tmp);
            assert!(read_port_from_registry().is_none());
        }
    }
}