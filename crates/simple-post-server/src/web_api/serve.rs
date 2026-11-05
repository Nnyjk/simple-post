//! `serve-web` 的启动入口。
//!
//! `serve` 绑定监听器、决定是否预热 MCP daemon，并把主端口的监听器交给 axum。

use std::path::PathBuf;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db;
use crate::error::AppError;
use crate::port_registry;

use super::router::{build_router, WebState};

/// 绑定 TCP listener 并服务到 SIGINT/SIGTERM。
///
/// `data_dir` 供 daemon supervisor 定位 SQLite 文件，必须与桌面端 `port_registry::data_dir()` 的
/// 解析结果一致（`SIMPLE_POST_DATA_DIR`，未设时是 `dirs::data_dir()`），两种形态才共用同一数据库。
/// `auto_start_daemon` 为 `Some(bool)` 时 CLI 已给定答案，`None` 时由持久化的 `mcp.auto_start`
/// 行决定。在这里解析而非在 `main.rs`：那边会另开一个 `db::init()` 池只为读一个 preference。
/// **MCP 的端点是守护进程自己的监听端口**，由持久化的 `mcp.http_config` 决定：守护进程启动时
/// 把那一行的绑定地址、端口、挂载路径与两张白名单读进命令行（见
/// `daemon_supervisor::daemon_cli_args`）。
///
/// 主端口**不**提供 MCP，故本进程不再另开一个「只服务 MCP」的监听器：那个监听器会抢走
/// 配置里写的端口（守护进程绑不上那个端口），而且它绑的是 Web 主机的地址，与配置里的
/// 绑定地址不是同一件事——两处口径不一致，端口与地址迟早对不上。
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
    // 口令一并交给它：守护进程是 MCP 唯一的对外端点，必须和主服务器用同一道 bearer 闸。
    let daemon = McpDaemonHandle::with_pool(pool.clone(), password.clone());

    // 在两个值被移进 `WebState` 之前定好守护进程决策：省掉第二次 `db::init()`，
    // 且后台任务预热的 handle 就是 `/api/mcp/*` 读写的那一个。
    let want_daemon = match auto_start_daemon {
        Some(v) => v,
        // 读不出偏好行时按「要起」处理：MCP 的端点只有守护进程一个，主端口不再提供 MCP，
        // 所以守护进程不起来就等于 MCP 消失。
        None => db::with_conn(&pool, db::preferences::mcp_auto_start).unwrap_or(true),
    };

    let state = WebState {
        pool,
        daemon: daemon.clone(),
        data_dir: data_dir.clone(),
        password,
        base_path,
    };
    let router = build_router(state, spa_dir);

    eprintln!("[serve-web] listening on http://{bind_addr}");
    eprintln!("[serve-web] data dir: {}", data_dir.display());

    // 必须先 bind 再预热 MCP daemon。预热要么收养端口文件里那个已经在服务的
    // daemon（一次 healthz 探测），要么拉起本可执行文件的 `mcp` 模式子进程（固定
    // 200ms sleep + Windows 上两次 `tasklist` + 一次 healthz）；排在 bind 之前，
    // 浏览器全程对着 connection refused，表现为白屏。
    //
    // 先 bind 则它变成真正的后台任务：daemon 与页面并行起来，
    // `/api/mcp/status` 在就绪前报 `Starting`——GUI 已在暴露这个瞬态。
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Validation(format!("bind {bind_addr}: {e}")))?;

    // MCP 的端点是守护进程自己的监听端口，不是这个监听器：守护进程作为后台任务被拉起，
    // 绑的是 `mcp.http_config` 里写的地址与端口。
    if want_daemon {
        let dir = data_dir.clone();
        tokio::spawn(async move {
            // 复用优先：端口文件里那个 daemon 服务的配置与本进程要用的这份一致时，
            // 收养它，不打断进行中的 MCP 会话（桌面应用起的 daemon 也会被收养）。
            // 配置不一致（或旧记录里没有指纹，判断不了）时才重启它。
            if let Err(e) = daemon.start_without_app_handle(&dir).await {
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

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "serve_test.rs"]
mod tests;
