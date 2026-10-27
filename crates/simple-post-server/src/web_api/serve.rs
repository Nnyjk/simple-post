//! `serve-web` 的启动入口。
//!
//! `serve` 绑定监听器、决定是否预热 MCP daemon，并把主端口与 MCP 专用端口两个
//! 监听器交给 axum。

use std::path::PathBuf;
use std::sync::Arc;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db;
use crate::error::AppError;
use crate::mcp::AppState as McpAppState;
use crate::port_registry;

use super::router::{build_mcp_only_router, build_router, WebState};

/// 绑定 TCP listener 并服务到 SIGINT/SIGTERM。
///
/// `data_dir` 供 daemon supervisor 定位 SQLite 文件，必须与桌面端
/// `dirs::data_dir().join("com.simple-post")` 的约定一致，两种形态才共用同一
/// 数据库。
///
/// `auto_start_daemon` 为 `Some(bool)` 时 CLI 已给定答案，`None` 时由持久化的
/// `mcp.auto_start` 行决定。在这里解析而非在 `main.rs`：那边会另开一个
/// `db::init()` 池只为读一个 preference，冷启动就付两次 schema 检查与两个连接池。
///
/// **MCP 专用端口由持久化的 `mcp.http_config` 决定**：`port` 非 0 时额外开一个
/// 只服务 MCP 的监听器（见 [`build_mcp_only_router`]），那个端口上没有 REST、
/// 没有 SPA，只有 MCP 自己的那组路由；`port` 为 0 时不开，主端口上的 `/mcp`
/// 照旧可用。
///
/// 端口取设置里的那一个，而不是另设一个环境变量：设置面板「连接 → HTTP 服务
/// 配置」里写的端口，就是对外暴露的那个端口。两处各写一个数字，迟早会对不上。
pub async fn serve(
    bind_addr: std::net::SocketAddr,
    data_dir: PathBuf,
    spa_dir: PathBuf,
    auto_start_daemon: Option<bool>,
    password: Option<String>,
    base_path: Option<String>,
) -> Result<(), AppError> {
    let pool = db::init(&data_dir)?;
    // 守护进程在 spawn 时从这个池读 `mcp.http_config`，使子进程命令行与设置面板一致。
    let daemon = McpDaemonHandle::with_pool(pool.clone());

    // 在两个值被移进 `WebState` 之前定好守护进程决策：省掉第二次 `db::init()`，
    // 且后台任务预热的 handle 就是 `/api/mcp/*` 读写的那一个。
    let want_daemon = match auto_start_daemon {
        Some(v) => v,
        None => db::with_conn(&pool, db::preferences::mcp_auto_start).unwrap_or(false),
    };

    // `/mcp` JSON-RPC 与本进程共用 `mcp_state`：pool 与 GUI 的 `simple-post.db`
    // 同源，metrics 按进程新建故每次重启归零。
    let mcp_state = Arc::new(McpAppState {
        pool: pool.clone(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    });

    // MCP 专用监听器的配置：与设置面板那张「HTTP 服务配置」卡片读的是同一行。
    //
    // `db::with_conn` 读不出这一行时按未配置处理（端口 0 → 不开）：这一行坏了
    // 不该让整个 Web 主机起不来，主端口上的 `/mcp` 仍然照常工作。
    let mcp_config = db::with_conn(&pool, db::preferences::mcp_http_config).unwrap_or_default();

    let state = WebState {
        pool,
        daemon: daemon.clone(),
        data_dir: data_dir.clone(),
        mcp_state,
        password,
        base_path,
    };
    // 专用 MCP 监听器的 router 必须在 `state` 被 `build_router` 移走之前做好。
    let mcp_only_router =
        (mcp_config.port > 0).then(|| build_mcp_only_router(state.clone(), &mcp_config));
    let router = build_router(state, spa_dir);

    eprintln!("[serve-web] listening on http://{bind_addr}");
    eprintln!("[serve-web] data dir: {}", data_dir.display());

    // 必须先 bind 再预热 MCP daemon。`restart_without_app_handle` 是固定 200ms
    // sleep + Windows 上两次 `tasklist` + 拉起本可执行文件的 `mcp` 模式子进程 +
    // 一次 healthz；排在 bind 之前，浏览器全程对着 connection refused，表现为
    // 白屏。
    //
    // 先 bind 则它变成真正的后台任务：daemon 与页面并行起来，
    // `/api/mcp/status` 在就绪前报 `Starting`——GUI 已在暴露这个瞬态。
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Validation(format!("bind {bind_addr}: {e}")))?;

    // 第二个监听器：只服务 MCP。两个监听器都在 daemon 预热之前 bind 好，
    // 所以「端口起不来」是一次启动期的硬错误，而不是运行期某个请求才发现的意外。
    //
    // 绑定失败即整体失败：设置里明确写了这个端口，起不来就该在启动时喊出来，
    // 而不是让容器以「起来了但少了那个端口」的样子被健康检查判为正常。
    //
    // 绑定地址取主监听器的地址，不取设置里的 `host`：设置里的那个地址是
    // **MCP daemon** 的绑定地址，而 daemon 在容器里拿不到宿主网卡的地址
    // （填宿主 IP 会 bind 失败）。专用端口与 Web 主机绑在同一个地址上，
    // 谁该访问它由 Host/Origin 守卫决定——那才是访问控制该待的地方。
    let mcp_listener = match mcp_only_router {
        Some(mcp_router) => {
            let mcp_addr = std::net::SocketAddr::new(bind_addr.ip(), mcp_config.port);
            let listener = tokio::net::TcpListener::bind(mcp_addr)
                .await
                .map_err(|e| AppError::Validation(format!("bind MCP listener {mcp_addr}: {e}")))?;
            eprintln!(
                "[serve-web] MCP-only listener on http://{mcp_addr}{} (from mcp.http_config)",
                mcp_config.normalized_path()
            );
            Some((listener, mcp_router))
        }
        None => None,
    };

    if let Some((listener, mcp_router)) = mcp_listener {
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, mcp_router).await {
                eprintln!("[serve-web] MCP-only listener exited with error: {e}");
            }
        });
    }

    if want_daemon {
        let dir = data_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = daemon.restart_without_app_handle(&dir).await {
                eprintln!("[serve-web] ensure_started failed: {e:?}");
            }
        });
    } else {
        eprintln!(
            "[serve-web] mcp.auto_start is off — daemon left stopped; \
             use POST /api/mcp/start or the settings panel to enable."
        );
    }

    axum::serve(listener, router)
        .await
        .map_err(|e| AppError::Validation(format!("axum::serve: {e}")))
}

/// 从当前可执行文件向上找 `dist/`。`simple-post.exe` 位于
/// `<project_root>/src-tauri/target/…`，向上若干级即到项目根；找不到时回落
/// `<cwd>/dist`。
pub fn default_spa_dir() -> PathBuf {
    // 最多向上找 6 级，够 `target/debug/` 与 `target/release/` 两种布局。
    let mut here = std::env::current_exe().ok();
    for _ in 0..6 {
        let Some(p) = here.as_ref() else { break };
        let candidate = p.parent().map(|d| d.join("dist"));
        if let Some(c) = candidate {
            if c.is_dir() {
                return c;
            }
        }
        here = p.parent().map(|x| x.to_path_buf());
    }
    // 回落 `cwd/dist`（在项目根执行 `cargo run` 时命中）。
    std::env::current_dir()
        .map(|d| d.join("dist"))
        .unwrap_or_else(|_| PathBuf::from("dist"))
}

/// 数据目录。与 `port_registry::data_dir()` 同一解析结果——守护子进程与本进程
/// 必须落在同一处，`mcp.port` 才发现得到。
pub fn default_data_dir() -> Result<PathBuf, AppError> {
    port_registry::data_dir()
}
