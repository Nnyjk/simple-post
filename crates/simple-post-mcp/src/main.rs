// crates/simple-post-mcp/src/main.rs
//
// 独立 MCP 入口。以 stdin/stdout 上的 JSON-RPC 对接运行中的
// `simple-post serve-web`。
//
// 用法：
//   simple-post-mcp                                       # stdio to localhost:4224
//   simple-post-mcp --url=http://B:4224 --password=<tok>  # cross-machine
//   simple-post-mcp --transport=http --port=4333         # expose /mcp over HTTP
//   simple-post-mcp --capability=read_only               # belt-and-suspenders filter
//
// 每个 flag 都有环境变量兜底（SIMPLE_POST_WEB_URL /
// SIMPLE_POST_WEB_PASSWORD / SIMPLE_POST_MCP_CAPABILITY），两者同时给时 flag 优先。

use std::io::IsTerminal;
use std::process::ExitCode;
use std::str::FromStr;

use clap::Parser;

use simple_post_mcp::capability::CapabilityOverride;
use simple_post_mcp::http_client::WebClient;

#[derive(Parser, Debug)]
#[command(
    name = "simple-post-mcp",
    version,
    about = "Standalone MCP bridge for Simple Post (stdio/HTTP client → simple-post-mcp daemon)"
)]
struct Cli {
    /// URL of the running `simple-post serve-web` instance.
    /// Env override: `SIMPLE_POST_WEB_URL`.
    #[arg(
        long,
        env = "SIMPLE_POST_WEB_URL",
        default_value = "http://localhost:4224"
    )]
    url: String,

    /// Optional bearer password for password-protected web backends.
    /// Env override: `SIMPLE_POST_WEB_PASSWORD`.
    #[arg(long, env = "SIMPLE_POST_WEB_PASSWORD")]
    password: Option<String>,

    /// Transport: `stdio` (default, line-oriented JSON-RPC on
    /// stdin/stdout) or `http` (exposes `/mcp` over HTTP for
    /// curl checks and `--transport=http` style clients).
    #[arg(long, default_value = "stdio", value_parser = ["stdio", "http"])]
    transport: String,

    /// Port for `--transport=http`. Ignored on stdio.
    #[arg(long, default_value_t = 4333)]
    port: u16,

    /// Local narrowing of the tool list. The backend already filters
    /// by its persisted `mcp.policy`; this flag lets the client
    /// demand an even stricter view. Default: backend decides.
    /// Env override: `SIMPLE_POST_MCP_CAPABILITY`.
    #[arg(long, env = "SIMPLE_POST_MCP_CAPABILITY")]
    capability: Option<String>,

    /// Log verbosity for the proxy's own diagnostics on stderr.
    /// JSON-RPC traffic stays on stdout regardless.
    #[arg(long, default_value = "info", value_parser = ["info", "debug"])]
    log_level: String,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // 装 tracing，`--log-level=debug` 才有 stderr 输出。装不上也不致命
    // （例如已被外层测试 harness 占了全局 subscriber）。
    init_tracing(&cli.log_level);

    let cap_override = match cli.capability.as_deref() {
        None => CapabilityOverride::FullAccess, // 本地不过滤，交后端裁决
        Some(s) => match CapabilityOverride::from_str(s) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[simple-post-mcp] --capability: {e}");
                return ExitCode::from(2);
            }
        },
    };

    let client = WebClient::new(cli.url.clone(), cli.password.clone());
    if std::env::var_os("SIMPLE_POST_MCP_DEBUG_HANDSHAKE").is_some() {
        match client.healthz().await {
            Ok(v) => eprintln!("[simple-post-mcp] handshake ok: {v}"),
            Err(e) => eprintln!("[simple-post-mcp] handshake failed: {e:?}"),
        }
    }

    match cli.transport.as_str() {
        "stdio" => {
            if std::io::stdin().is_terminal() {
                eprintln!(
                    "[simple-post-mcp] stdin is a TTY — launching stdio mode interactively \
                     is not supported. Wire this binary into your MCP client config instead \
                     (Claude Desktop / Cursor / etc.)"
                );
                return ExitCode::from(2);
            }
            let log = std::io::stderr();
            match simple_post_mcp::stdio_loop::run(client, cap_override, log).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("[simple-post-mcp] stdio loop terminated: {e}");
                    ExitCode::from(1)
                }
            }
        }
        "http" => {
            // 起一个极小的 axum 面，让 curl / Node 冒烟测试不必拉起整套
            // `serve-web` + daemon 就能 POST /mcp。转发用的是与 stdio 模式
            // 同一个 `WebClient`。
            run_http_transport(client, cap_override, cli.port).await
        }
        other => {
            eprintln!("[simple-post-mcp] unknown --transport: {other}");
            ExitCode::from(2)
        }
    }
}

fn init_tracing(level: &str) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(match level {
            "debug" => "simple_post_mcp=debug,info",
            _ => "simple_post_mcp=info,warn",
        })
    });
    let _ = tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .try_init();
}

async fn run_http_transport(client: WebClient, _cap: CapabilityOverride, port: u16) -> ExitCode {
    use axum::{routing::get, routing::post, Json, Router};
    use serde_json::Value;

    let state = client.clone();

    async fn post_mcp(
        axum::extract::State(client): axum::extract::State<WebClient>,
        Json(frame): Json<Value>,
    ) -> Json<Value> {
        match client.post_mcp(&frame).await {
            Ok(v) => Json(v),
            Err(e) => {
                let (code, msg) = simple_post_mcp::http_client::to_rpc_error(e);
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": frame.get("id").cloned().unwrap_or(Value::Null),
                    "error": { "code": code, "message": msg }
                }))
            }
        }
    }

    async fn healthz(axum::extract::State(client): axum::extract::State<WebClient>) -> Json<Value> {
        match client.healthz().await {
            Ok(v) => Json(v),
            Err(e) => Json(serde_json::json!({ "status": "down", "error": e.to_string() })),
        }
    }

    let app = Router::new()
        .route("/mcp", post(post_mcp))
        .route("/healthz", get(healthz))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    eprintln!("[simple-post-mcp] http transport listening on {addr}/mcp");
    match axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service(),
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[simple-post-mcp] http transport failed: {e}");
            ExitCode::from(1)
        }
    }
}
