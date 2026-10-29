// release 构建下 Windows 不再弹多余的控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, Subcommand};

/// Command-line entry point for `simple-post`.
///
/// Subcommands:
///   - (no subcommand) → launch the Tauri GUI
///   - `mcp`           → run as MCP daemon (HTTP transport by default)
///   - `mcp-proxy`     → run as stdio↔HTTP proxy (separate binary; refused)
///   - `serve-web`     → run the browser-facing REST + SPA host on a
///     LAN-reachable port (the data lives on this host; the browser
///     only sees the HTTP surface).
#[derive(Parser, Debug)]
#[command(
    name = "simple-post",
    version,
    about = "Modern desktop API tool with built-in MCP"
)]
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

        /// Mount path for the JSON-RPC endpoint. Must start with `/`.
        /// The canonical `/mcp` stays reachable as an alias, so AI
        /// clients already configured for it keep working.
        #[arg(long, default_value = "/mcp")]
        path: String,

        /// Answer requests whose `Host` is not loopback. Off by default:
        /// with it off, a non-loopback `Host` gets 403. Turning it on
        /// without adding anything to `--allowed-hosts` therefore denies
        /// every remote host — "no allowlist" means "nothing extra",
        /// never "everything".
        #[arg(long)]
        allow_remote: bool,

        /// Comma-separated `Host` values accepted once `--allow-remote`
        /// is on. `*` matches anything. Empty (the default) accepts no
        /// remote host.
        ///
        /// Declared as `Option<String>` rather than `Vec<String>` with
        /// `value_delimiter`: the supervisor emits `--allowed-hosts=`
        /// with an *empty* value for an empty list, and clap's delimiter
        /// handling of an empty value is exactly the kind of detail the
        /// daemon's ability to start must not depend on. The split
        /// happens in [`split_allow_list`], where it is ours.
        #[arg(long)]
        allowed_hosts: Option<String>,

        /// Comma-separated `Origin` values accepted. A request with no
        /// `Origin` (curl, Claude Code, Claude Desktop) is always
        /// allowed; one that sends it must match here. `*` matches
        /// anything.
        #[arg(long)]
        allowed_origins: Option<String>,
    },

    /// Run as stdio→HTTP proxy. This is a separate binary
    /// (`simple-post-mcp.exe`); the main binary prints an error and exits.
    /// Use the dedicated binary directly, or point the client at the
    /// daemon over HTTP (no local install needed on the client machine).
    #[command(name = "mcp-proxy")]
    McpProxy,

    /// Run the browser-facing web server (REST + SPA). The browser
    /// never spawns an MCP daemon in this shape — all tool calls are
    /// delegated to this host, which serves the same SQLite file and
    /// the same preferences as the desktop app.
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

        /// Data directory. Defaults to `dirs::data_dir()` (or
        /// `SIMPLE_POST_DATA_DIR` when that is set) so the GUI and the
        /// server share the same SQLite file.
        #[arg(long)]
        data_dir: Option<String>,

        /// Override the persisted `mcp.auto_start` preference for this
        /// launch only. `--mcp-auto-start` forces the daemon to come up
        /// (and updates the row); `--no-mcp-auto-start` forces it to
        /// stay down. If neither flag is given, the persisted value is
        /// honoured.
        #[arg(long)]
        mcp_auto_start: Option<bool>,

        /// Bearer password for `/api/*` + `/mcp` requests. When omitted
        /// (or empty) the route tree stays open, which is the
        /// backwards-compatible default. When set,
        /// every `/api/*` and `/mcp` request must carry
        /// `Authorization: Bearer <token>`; `/api/healthz`,
        /// `/mcp/healthz`, `/healthz`, and the SPA shell stay open
        /// so probes and the SPA bootstrap keep functioning.
        /// Also accepted from `SIMPLE_POST_WEB_PASSWORD`; the flag
        /// wins when both are set.
        #[arg(long, env = "SIMPLE_POST_WEB_PASSWORD")]
        web_password: Option<String>,

        /// Reverse-proxy base path. When set, the entire
        /// router is `nest`ed under this prefix so the SPA + REST +
        /// `/mcp` can live behind e.g. nginx at
        /// `https://host/sp/`. Must start with `/`. Also accepted
        /// from `SIMPLE_POST_PUBLIC_BASE_PATH`; the flag wins.
        #[arg(long, env = "SIMPLE_POST_PUBLIC_BASE_PATH")]
        public_base_path: Option<String>,
    },
}

/// 把 `--allowed-*` 的命令行列表归一成真正的 `Vec<String>`。
///
/// `None`（flag 未给）与 `Some("")`（supervisor 对空列表发的 `--allowed-hosts=`）都必须
/// 落到空列表，绝不能落成单元素 `[""]`。空 host allowlist 等于 daemon 拒绝一切非
/// loopback `Host`，多出的那个空项既会漏进持久化配置，又是一条永远匹配不上的规则。
fn split_allow_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() {
    simple_post_lib::init_tracing();

    let cli = Cli::parse();
    match cli.cmd {
        Some(Cmd::Mcp {
            transport,
            port,
            bind,
            path,
            allow_remote,
            allowed_hosts,
            allowed_origins,
        }) => {
            // 转发给 lib 入口，二进制与 lib 共用同一份 daemon 实现。
            let http_config = simple_post_lib::domain::McpHttpConfig {
                host: bind.clone(),
                port,
                path,
                allow_remote,
                allowed_hosts: split_allow_list(allowed_hosts),
                allowed_origins: split_allow_list(allowed_origins),
            };
            simple_post_lib::run_mcp(transport, port, bind, http_config);
        }

        // mcp-proxy 是独立二进制，此处拒绝启动。
        Some(Cmd::McpProxy) => {
            eprintln!(
                "mcp-proxy is a separate binary:\n  \
                 - cargo-installed: cargo install simple-post-mcp\n  \
                 - or connect the client to the daemon over HTTP instead"
            );
            std::process::exit(2);
        }

        // 面向浏览器的 web 宿主：起一个小 tokio runtime、解析 bind 地址，转发给 lib 入口。
        Some(Cmd::ServeWeb {
            bind,
            port,
            spa_dir,
            data_dir,
            mcp_auto_start,
            web_password,
            public_base_path,
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

            // 命令行给了就覆盖持久化的那一行。
            //
            // `None` 原样转发，不在此解析：为此单独开一个 `db::init()` 池只为读一个
            // preference（一次 schema 检查、整份 `schema.sql`、4 连接池），而几行之后
            // `web_api::serve` 会为同一个文件再开一个；`serve()` 用它已有的池解析该值。
            //
            // 显式给值时仍在此落库，强制值因此被持久化。
            let auto_start = match mcp_auto_start {
                Some(v) => {
                    // 落库覆盖值，后续启动才一致。
                    if let Ok(pool) = simple_post_lib::db::init(&data_path) {
                        let _ = simple_post_lib::db::with_tx(&pool, |c| {
                            simple_post_lib::db::preferences::set(
                                c,
                                simple_post_lib::db::preferences::keys::MCP_AUTO_START,
                                &v,
                            )
                        });
                    }
                    Some(v)
                }
                None => None,
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
                web_password,
                public_base_path,
            ));
        }

        None => simple_post_lib::run(),
    }
}

/// 内联而非另立 `main_test.rs`：本二进制没有自己的测试模块，而这里唯一值得钉住的是
/// 「clap 解析出的 flag ↔ daemon 实际服务的配置」这一往返。
/// `daemon_supervisor::daemon_cli_args` 正是发出这些字符串去拉 daemon 子进程的，一旦
/// 改名或空列表解析方式变了，daemon 起不来，而 supervisor 只会报 `Unhealthy`。
#[cfg(test)]
mod tests {
    use super::*;

    /// supervisor 在空 allowlist 下构造的 argv。clap 一旦拒绝这个空值，这里就解析失败，
    /// daemon 子进程永远起不来。
    const SUPERVISOR_ARGV: &[&str] = &[
        "simple-post",
        "mcp",
        "--transport=http",
        "--port=43123",
        "--bind=127.0.0.1",
        "--path=/mcp",
        "--allowed-hosts=",
        "--allowed-origins=",
    ];

    fn parse_mcp(argv: &[&str]) -> (simple_post_lib::domain::McpHttpConfig, bool) {
        match Cli::try_parse_from(argv).expect("must parse").cmd {
            Some(Cmd::Mcp {
                bind,
                path,
                allow_remote,
                allowed_hosts,
                allowed_origins,
                ..
            }) => (
                simple_post_lib::domain::McpHttpConfig {
                    host: bind,
                    path,
                    allow_remote,
                    allowed_hosts: split_allow_list(allowed_hosts),
                    allowed_origins: split_allow_list(allowed_origins),
                    ..simple_post_lib::domain::McpHttpConfig::default()
                },
                allow_remote,
            ),
            other => panic!("expected the mcp subcommand, got {other:?}"),
        }
    }

    #[test]
    fn the_flags_the_supervisor_emits_parse_back_to_the_same_config() {
        let (cfg, allow_remote) = parse_mcp(SUPERVISOR_ARGV);
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.path, "/mcp");
        assert!(!allow_remote);
        assert!(
            cfg.allowed_hosts.is_empty(),
            "an empty allowlist must not arrive as [\"\"] — it is a deny-all, not a rule"
        );
        assert!(cfg.allowed_origins.is_empty());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn an_omitted_allowlist_flag_equals_an_empty_one() {
        let (cfg, _) = parse_mcp(&["simple-post", "mcp"]);
        assert!(cfg.allowed_hosts.is_empty());
        assert!(cfg.allowed_origins.is_empty());
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.path, "/mcp");
    }

    #[test]
    fn a_comma_separated_list_becomes_one_entry_per_item() {
        let (cfg, allow_remote) = parse_mcp(&[
            "simple-post",
            "mcp",
            "--bind=0.0.0.0",
            "--path=/ai/mcp",
            "--allow-remote",
            "--allowed-hosts=studio.local, 10.0.0.5 ,",
            "--allowed-origins=https://claude.ai",
        ]);
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.path, "/ai/mcp");
        assert!(allow_remote);
        assert_eq!(cfg.allowed_hosts, vec!["studio.local", "10.0.0.5"]);
        assert_eq!(cfg.allowed_origins, vec!["https://claude.ai"]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn a_lan_bind_without_allow_remote_is_rejected_by_validation() {
        // 否则就会一边监听 LAN，一边被 Host 守卫对一切外来请求回 403。
        let (cfg, _) = parse_mcp(&[
            "simple-post",
            "mcp",
            "--bind=0.0.0.0",
            "--allowed-hosts=studio.local",
        ]);
        assert!(cfg.validate().is_err());
    }
}
