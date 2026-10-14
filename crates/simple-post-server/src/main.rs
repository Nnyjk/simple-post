// `simple-post-server` — the headless entrypoint.
//
// This binary is what the Docker image runs. It is the same CLI shape
// as the `serve-web` / `mcp` subcommands of the desktop `simple-post`
// binary, but built without Tauri, so it carries no GTK/WebKit on
// Linux and no Windows GUI runtime.
//
// `serve-web` is the container command:
//
//     simple-post-server serve-web --bind 0.0.0.0 --port 4173
//
// There is deliberately no "no subcommand" case: launching the desktop
// GUI is not something this binary can do, and defaulting to an empty
// help screen is friendlier than a panic.

use clap::{Parser, Subcommand};

/// Headless server host for Simple Post.
///
/// This is the Tauri-free half of the project: the browser-facing REST
/// and SPA server (`serve-web`), plus the embedded MCP daemon (`mcp`).
/// The desktop application lives in the `simple-post` binary.
#[derive(Parser, Debug)]
#[command(
    name = "simple-post-server",
    version,
    about = "Headless server host for Simple Post (REST + SPA + MCP)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the browser-facing web server (REST + SPA).
    #[command(name = "serve-web")]
    ServeWeb {
        /// Bind address. `127.0.0.1` keeps the server local-only;
        /// `0.0.0.0` exposes the SPA + REST API on the LAN. Containers
        /// need `0.0.0.0` — the default would publish a port nothing
        /// can reach.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        /// Port to listen on.
        #[arg(long, default_value_t = 4173)]
        port: u16,

        /// Path to the built SPA bundle. Defaults to `<project_root>/dist`.
        #[arg(long)]
        spa_dir: Option<String>,

        /// Data directory. Defaults to the OS-conventional
        /// `dirs::data_dir()/simple-post`. Mount this on a volume in a
        /// container: it holds the SQLite database, the `mcp.port`
        /// record, and every environment definition.
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
        /// backwards-compatible default — and, on a `0.0.0.0` bind,
        /// publishes the whole API to the network. Set it.
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
        #[arg(long)]
        allowed_hosts: Option<String>,

        /// Comma-separated `Origin` values accepted. A request with no
        /// `Origin` (curl, Claude Code, Claude Desktop) is always
        /// allowed; one that sends it must match here. `*` matches
        /// anything.
        #[arg(long)]
        allowed_origins: Option<String>,
    },
}

fn main() {
    simple_post_server::init_tracing();

    match Cli::parse().cmd {
        Cmd::ServeWeb {
            bind,
            port,
            spa_dir,
            data_dir,
            mcp_auto_start,
            web_password,
            public_base_path,
        } => serve_web(
            bind,
            port,
            spa_dir,
            data_dir,
            mcp_auto_start,
            web_password,
            public_base_path,
        ),

        Cmd::Mcp {
            transport,
            port,
            bind,
            path,
            allow_remote,
            allowed_hosts,
            allowed_origins,
        } => {
            let http_config = simple_post_server::domain::McpHttpConfig {
                host: bind.clone(),
                port,
                path,
                allow_remote,
                allowed_hosts: split_allow_list(allowed_hosts),
                allowed_origins: split_allow_list(allowed_origins),
            };
            simple_post_server::run_mcp(transport, port, bind, http_config);
        }
    }
}

/// Normalise a `--allowed-*` CLI list into a real `Vec<String>`.
///
/// `None` (flag absent) and `Some("")` (the supervisor's
/// `--allowed-hosts=` for an empty list) both have to land on an empty
/// list, never on a one-element `[""]`. An empty host allowlist is the
/// daemon *denying* every non-loopback `Host`, so a stray empty entry
/// would both leak into the persisted config and become a never-matching
/// rule.
fn split_allow_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Resolve `serve-web`'s flags into concrete paths and run the server.
fn serve_web(
    bind: String,
    port: u16,
    spa_dir: Option<String>,
    data_dir: Option<String>,
    mcp_auto_start: Option<bool>,
    web_password: Option<String>,
    public_base_path: Option<String>,
) {
    let bind_addr: std::net::SocketAddr = match format!("{bind}:{port}").parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[serve-web] invalid --bind/--port: {e}");
            std::process::exit(2);
        }
    };
    let spa_path = match spa_dir {
        Some(p) => std::path::PathBuf::from(p),
        None => simple_post_server::web_api::default_spa_dir(),
    };
    let data_path = match data_dir {
        Some(p) => std::path::PathBuf::from(p),
        None => match simple_post_server::web_api::default_data_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[serve-web] cannot resolve data dir: {e:?}");
                std::process::exit(2);
            }
        },
    };

    // The CLI flag, when present, overrides the persisted row.
    //
    // `None` is forwarded verbatim rather than being resolved here:
    // `web_api::serve` resolves the persisted value from the pool it
    // already opens, and opening a second one here just to read one
    // preference means a second schema check and a second connection
    // pool for the same file.
    //
    // An explicit flag still writes the override through here, so a
    // forced value is persisted exactly as before.
    let auto_start = match mcp_auto_start {
        Some(v) => {
            if let Ok(pool) = simple_post_server::db::init(&data_path) {
                let _ = simple_post_server::db::with_tx(&pool, |c| {
                    simple_post_server::db::preferences::set(
                        c,
                        simple_post_server::db::preferences::keys::MCP_AUTO_START,
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
    runtime.block_on(simple_post_server::run_serve_web(
        bind_addr,
        spa_path,
        data_path,
        auto_start,
        web_password,
        public_base_path,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Cli {
        Cli::try_parse_from(argv).expect("must parse")
    }

    #[test]
    fn an_omitted_allowlist_flag_equals_an_empty_one() {
        match parse(&["simple-post-server", "mcp"]).cmd {
            Cmd::Mcp {
                bind,
                port,
                path,
                allow_remote,
                allowed_hosts,
                allowed_origins,
                ..
            } => {
                assert!(allowed_hosts.is_none());
                assert!(allowed_origins.is_none());
                assert_eq!(bind, "127.0.0.1");
                assert_eq!(port, 0);
                assert_eq!(path, "/mcp");
                assert!(!allow_remote);
                // And the normalisation the dispatch arm applies:
                assert!(split_allow_list(allowed_hosts).is_empty());
                assert!(split_allow_list(allowed_origins).is_empty());
            }
            other => panic!("expected the mcp subcommand, got {other:?}"),
        }
    }

    /// The supervisor emits `--allowed-hosts=` with an *empty* value for
    /// an empty list. clap hands that over as `Some("")`, and it must
    /// normalise to an empty list rather than a one-element `[""]` — a
    /// literal empty string never matches any `Host`.
    #[test]
    fn a_supervisor_style_empty_allowlist_stays_empty() {
        let empty = Some(String::new());
        assert!(split_allow_list(empty).is_empty());
    }

    #[test]
    fn a_comma_separated_list_becomes_one_entry_per_item() {
        let hosts = split_allow_list(Some("studio.local, 10.0.0.5 ,".to_string()));
        assert_eq!(hosts, vec!["studio.local", "10.0.0.5"]);
        let origins = split_allow_list(Some("https://claude.ai".to_string()));
        assert_eq!(origins, vec!["https://claude.ai"]);
    }

    /// The container default. The Dockerfile passes an explicit
    /// `--bind 0.0.0.0`; if the *default* ever changed to a LAN bind
    /// instead, that flag would stop being a deliberate choice and the
    /// "no bind flag means loopback-only" guarantee would be gone.
    #[test]
    fn serve_web_defaults_are_the_documented_ones() {
        match parse(&["simple-post-server", "serve-web"]).cmd {
            Cmd::ServeWeb {
                bind,
                port,
                web_password,
                public_base_path,
                ..
            } => {
                assert_eq!(bind, "127.0.0.1");
                assert_eq!(port, 4173);
                assert!(web_password.is_none());
                assert!(public_base_path.is_none());
            }
            other => panic!("expected the serve-web subcommand, got {other:?}"),
        }
    }

    /// A container passes every flag explicitly. If clap's names drift,
    /// the image would silently start on defaults — most dangerously
    /// loopback-only, which publishes a port nothing can reach.
    #[test]
    fn the_container_flags_parse_back_to_themselves() {
        match parse(&[
            "simple-post-server",
            "serve-web",
            "--bind=0.0.0.0",
            "--port=4173",
            "--spa-dir=/app/dist",
            "--data-dir=/data",
            "--web-password=s3cret",
            "--public-base-path=/sp",
        ])
        .cmd
        {
            Cmd::ServeWeb {
                bind,
                port,
                spa_dir,
                data_dir,
                web_password,
                public_base_path,
                ..
            } => {
                assert_eq!(bind, "0.0.0.0");
                assert_eq!(port, 4173);
                assert_eq!(spa_dir.as_deref(), Some("/app/dist"));
                assert_eq!(data_dir.as_deref(), Some("/data"));
                assert_eq!(web_password.as_deref(), Some("s3cret"));
                assert_eq!(public_base_path.as_deref(), Some("/sp"));
            }
            other => panic!("expected the serve-web subcommand, got {other:?}"),
        }
    }

    /// There is no GUI here, so a bare invocation must be a parse
    /// error rather than a silent no-op.
    #[test]
    fn a_bare_invocation_is_rejected() {
        assert!(Cli::try_parse_from(["simple-post-server"]).is_err());
    }
}
