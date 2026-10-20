// ===========================================================================
// region: mcp_status
// ===========================================================================

use tauri::State;

use crate::error::AppError;

use super::dto::{McpLogTailDto, McpStatusDto, RestartResult};

/// MCP daemon 快照。读 Tauri state 里的内存 handle，完全不碰 DB。
/// `snapshot()` 必须返回 [`McpStatusDto`]。
#[tauri::command]
pub fn mcp_status(
    state: State<crate::daemon_supervisor::McpDaemonHandle>,
) -> Result<McpStatusDto, AppError> {
    Ok(state.snapshot())
}

/// daemon 最近若干行日志。读内存里的环形缓冲，完全不碰 DB。
///
/// 缓冲是**有界**的（最近 500 行），故这份东西永远不长。
#[tauri::command]
pub fn mcp_daemon_log_tail(
    state: State<crate::daemon_supervisor::McpDaemonHandle>,
) -> Result<McpLogTailDto, AppError> {
    Ok(state.log_tail())
}

/// AI 客户端在 `mcpServers` 里启动的 `simple-post-mcp` 绝对路径，供连接面板的
/// 「复制代理路径」直接粘贴。
///
/// 取 `current_exe()` 的所在目录：两个二进制经 `cargo build --workspace` 落在同一
/// `target/<profile>/`，没有 npm 包装路径可指。
///
/// 查找失败时返回 `"<unresolved current_exe>"`，渲染侧按不可操作的占位串处理，不阻塞 UI。
#[tauri::command]
pub fn mcp_daemon_proxy_path() -> String {
    use std::path::PathBuf;

    fn build(_exe_name: &str, proxy_name: &str) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post.exe", "simple-post-mcp.exe")
    } else {
        build("simple-post", "simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// 重启内嵌 MCP daemon。用户层面幂等：面板在重启进行中禁用按钮，连点两次只重启一次。
///
/// `AppError` 被吞成 `{ ok: false, pid: null, port: null }` 而不外抛：渲染侧的
/// restart 动作自带 try/catch 会把徽章翻成 `crashed`，统一 `{ ok: bool }` 信封
/// 让调用点形状与 start / stop 一致。
#[tauri::command]
pub async fn mcp_daemon_restart(
    state: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
) -> Result<RestartResult, AppError> {
    match state.restart(&app_handle).await {
        Ok(snapshot) => Ok(RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        }),
        Err(e) => {
            eprintln!("[mcp_daemon_restart] failed: {e:?}");
            Ok(RestartResult {
                ok: false,
                pid: None,
                port: None,
            })
        }
    }
}

/// 手动开：启动内嵌 MCP daemon。
///
/// 幂等：已跟踪到健康 daemon 时原样返回其 `pid` / `port`，不杀不重启；只有真的
/// 停止 / 崩溃 / 不健康才走拉起路径并报新的 daemon 身份。
///
/// 返回 [`RestartResult`] `{ ok, pid, port }`，与 restart / stop 同形，渲染侧不必
/// 按动词分支。底层 `AppError`（拉起失败、healthz 超时）同样吞成
/// `{ ok: false, pid: null, port: null }`。
///
/// 与 `mcp_auto_start_set` 的分工：那个管「启动时」，从不杀运行中的 daemon；本命令
/// 才是运行时开关。`auto_start` 打开会走同一条幂等路径把它拉起来，关闭只压制以后
/// 的启动——要现在停，得调本命令。
#[tauri::command]
pub async fn mcp_daemon_start(
    state: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
) -> Result<RestartResult, AppError> {
    match state.start(&app_handle).await {
        Ok(snapshot) => Ok(RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        }),
        Err(e) => {
            eprintln!("[mcp_daemon_start] failed: {e:?}");
            Ok(RestartResult {
                ok: false,
                pid: None,
                port: None,
            })
        }
    }
}

/// 手动关：终止内嵌 MCP daemon 并释放它的端口文件。
///
/// 本 supervisor 拉起的 daemon 和仅从端口记录「接手」的 daemon 都会终止，否则会留下
/// 占着端口的孤儿进程。没东西在跑时调用是安全的，返回 `{ ok: true, pid: null, port: null }`
/// 而非报错。
///
/// 返回 [`RestartResult`]，与 `mcp_daemon_restart` 逐字节同形。停成功后 `pid` 与
/// `port` 按定义即 `null`，随后的 `mcp_daemon_start` 干净地走新拉起路径。
///
/// 与 `mcp_auto_start_set` 的分工：关 `auto_start` 不动运行中的 daemon，只有本命令
/// 能现在停。
#[tauri::command]
pub async fn mcp_daemon_stop(
    state: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
) -> Result<RestartResult, AppError> {
    match state.stop().await {
        Ok(snapshot) => Ok(RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        }),
        Err(e) => {
            eprintln!("[mcp_daemon_stop] failed: {e:?}");
            Ok(RestartResult {
                ok: false,
                pid: None,
                port: None,
            })
        }
    }
}
