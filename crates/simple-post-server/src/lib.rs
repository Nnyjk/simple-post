//! Simple Post 的无头服务端 host。
//!
//! 本 crate 是桌面 shell 与容器镜像共享的一切，减去 GUI。它拥有：
//!
//!   * `web_api` —— `serve-web` 背后的 axum router：REST 面、`/mcp` JSON-RPC 端点、
//!     静态 SPA host。
//!   * `daemon_supervisor` —— 与 host 无关的 MCP daemon spawn / teardown 逻辑。
//!   * `auth` —— bearer 中间件及其公开路径名单。
//!   * `port_registry` —— stdio bridge 用来发现端口的 `mcp.port` 文件。
//!   * `dto` / `http_config` —— 两条 transport 共享的线格式类型与 daemon 配置辅助函数。
//!
//! ## 为何要单独一个 crate
//!
//! `src-tauri` 无条件链接 `tauri`，而 Tauri 在 Linux 上会拖来整个 GTK/WebKit 栈。
//! 这让「只跑服务端」无法容器化：镜像为一个永远显示不出来的 GUI 背上 X11 库，
//! `--lib` 测试二进制还通过 `AppHandle` 把 Windows GUI runtime 拖了进来（见
//! `daemon_supervisor` 上的长注记）。把服务端拆出来，镜像就是 `debian-slim` 加一个
//! 二进制，测试二进制也不会链接它不该链接的东西。
//!
//! 依赖方向是单向的：
//!
//! ```text
//! simple-post (Tauri)  ->  simple-post-server  ->  simple-post-core
//!                                     |
//!                                     +-------------->  simple-post-mcp
//! ```
//!
//! `src-tauri` 再导出下列模块，故 Tauri 命令里既有的
//! `crate::daemon_supervisor::…` / `crate::web_api::…` 路径仍能解析。

pub mod auth;
pub mod daemon_supervisor;
pub mod dto;
/// 动态变量的编排层。Tauri 命令与 REST 路由都只调它，故两条 transport 不必各写一遍
/// ——见 AGENTS.md 的 dual-transport invariant。
pub mod dynamic_api;
pub mod http_config;
/// 导入 / 导出的编排层。Tauri 命令与 REST 路由都只调它，故两条 transport 不必
/// 各写一遍——见 AGENTS.md 的 dual-transport invariant。
pub mod interchange_api;
pub mod port_registry;
pub mod web_api;

// 再导出共享的底层，好让调用点在 workspace 抽取与本次 crate 拆分之后继续使用
// `crate::db::*` / `crate::domain::*` / `crate::error::*` /
// `crate::permissions::*` / `crate::http_executor::*` /
// `crate::mcp_metrics::*` / `crate::mcp::*`。
pub use simple_post_core::{
    db, domain, error, http_executor, mcp_metrics, oauth2, permissions, script,
};

// 转换层本身也再导出一次，好让调用点写 `crate::interchange::…` 而不是
// `simple_post_interchange::…`——与上面那两行的理由相同。
pub use simple_post_interchange as interchange;

// 再导出独立 `simple-post-mcp` crate 的 MCP 模块。之所以也再导出 `daemon_http`，
// 是因为 `run_mcp` 需要 `serve_with_config` 把监听配置（挂载路径 + Host/Origin
// 白名单）交给 router，而 `mcp::serve_http` 没有这些参数。
pub use simple_post_mcp::{daemon_http, mcp};

/// CLI 子命令 `mcp` 的入口。负责数据目录解析、DB 初始化、axum serve、端口记录写入，
/// 以及 SIGINT/SIGTERM 关闭。
///
/// `http_config` 承载 router 需要的、超出 `bind` / `port` 之外的监听设置：挂载路径
/// 与 Host/Origin 白名单。supervisor 从 `mcp.http_config` 偏好填它（见
/// `daemon_cli_args`）；手工跑的 `simple-post mcp` 拿的是 clap 默认值，也就是出厂的
/// 仅 loopback 配置。
pub fn run_mcp(
    transport: String,
    port: u16,
    bind: String,
    http_config: crate::domain::McpHttpConfig,
) {
    init_tracing();

    // 手工跑的 daemon 也可能被指向矛盾的配置（`--bind=0.0.0.0` 却不带
    // `--allow-remote`）。在此失败远好过起一个监听在网络上、却被 Host 守卫一律回
    // 403 的 daemon。
    if let Err(e) = http_config.validate() {
        eprintln!("[mcp] invalid listener configuration: {e}");
        std::process::exit(2);
    }

    // 与 `port_registry` *完全同样*地解析数据目录，好让 daemon 读写的 SQLite 文件就
    // 在 proxy 发现的 `mcp.port` 文件旁边。两者一起认 `SIMPLE_POST_DATA_DIR`。
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
        // 先开 SQLite 池，好让 daemon 的工具在第一个请求上就能读/写，不付首次调用的
        // 预热成本。
        let pool = match crate::db::init(&data_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[mcp] db::init({}) failed: {e:?}", data_dir.display());
                std::process::exit(1);
            }
        };

        // 同时支持 `--transport=stdio` 与默认的 `--transport=http`，但一律走同一个 axum
        // router，这样只维护一套 handler。（推荐的 stdio 路径是 proxy 二进制；这个
        // 回落分支主要是为了让该 CLI 参数存在——若将来 AI 客户端支持直连 HTTP MCP
        // 就能启用。）
        let _ = transport; // 为向前兼容而接受，目前尚未分支处理。

        // 这里直接 bind + spawn 而不走 `mcp::serve_http`：后者的签名早于监听配置，
        // 没法把挂载路径 / 白名单传进 `daemon_http::serve`。下面的 block 是同样的
        // bind → 解析端口 → spawn 序列，只是把 `serve` 换成 `serve_with_config`。
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

        // 发布实时端口，好让 `simple-post-mcp` 及任何 AI 客户端能找到我们。CLI 模式是
        // 测试与无头自动化运行 daemon 的正统方式；不写这一条，proxy 会拒绝启动
        // （"daemon not running"）。
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

        // 阻塞到 SIGINT/SIGTERM。`tokio::signal` 既返回 Ctrl-C/SIGINT 的 future，也返回
        // 非 Windows 上的独立 SIGTERM 处理器；Windows 上只有 Ctrl-C 会被有效投递。
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
        // 尽力删掉端口文件，免得下一次 `simple-post.exe mcp`（或 GUI supervisor）看到
        // 陈旧记录。
        let _ = crate::port_registry::release();
    });
}

/// CLI 子命令 `mcp-proxy` 的入口。proxy 是独立二进制
/// （`simple-post-mcp[.exe]`）；用主二进制跑它几乎肯定是用户搞错了（PATH 不对、
/// 快捷方式指错），故指路到正确的二进制并以非零退出，好让 CI 脚本能抓到。
pub fn run_mcp_proxy() {
    eprintln!(
        "mcp-proxy is a separate binary — please run `simple-post-mcp` instead \
         (or connect the client to the daemon over HTTP)."
    );
    std::process::exit(2);
}

/// CLI 子命令 `serve-web` 的入口。拉起一个 axum HTTP 服务器，它：
///
///   * 从 `<project>/dist/` 提供打包好的 SPA；
///   * 暴露 `/api/*` REST 路由供浏览器 CRUD + HTTP 发送 + prefs；
///   * 让既有的 `/mcp` JSON-RPC router 原封不动，使 MCP 客户端
///     （Claude Desktop、Cursor 等）照常工作。
///
/// 浏览器从不 spawn daemon——它直连 `127.0.0.1:<port>`。daemon 子进程（若
/// `mcp.auto_start == true`）跑在本机 `simple-post` 进程树下，只能经 REST 桥接触达；
/// AI 客户端要么连自己机器上的 `simple-post-mcp` 二进制，要么直连本机的 HTTP 端点。
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

/// 初始化全局 `tracing` subscriber，好让全 crate 的 `tracing::info!` /
/// `tracing::warn!` / `tracing::error!` 宏真正输出。
///
/// 认 `RUST_LOG`（经 `EnvFilter`），未设置时默认 `info`。用 `try_init`，重复初始化
/// （测试 + GUI + CLI）会静默 no-op 而不是 panic。
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
