//! 退出监视器：子进程死了就把状态翻成 `Crashed`，但绝不覆盖一次有意的 teardown。
//!
//! 两个变体只差一个 `daemon://crashed` 事件：Tauri 形态发，web 形态不发——
//! 后者靠 `/api/mcp/status` 轮询得到同一事实。

use std::sync::Arc;

use tokio::sync::Mutex;

#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter};

use super::state::DaemonState;
#[cfg(feature = "gui")]
use super::status::DaemonCrashedPayload;
use super::status::DaemonStatus;

/// 纯函数：这个退出监视器是否仍拥有监管者状态。
///
/// 两个 `watch_exit` 变体都只在自己监视的 daemon 仍是状态所指的那个时才把共享
/// 状态翻成 `Crashed`。`teardown` 会**先**清 `pid` 再 kill，所以被 teardown 自己
/// 那次 kill 唤醒的监视器看到 `None`，保持安静，不会覆盖动词刚设的
/// `Restarting` / `Stopped`，更不会覆盖后续 start 在同一 handle 上装的新状态。
/// 保持成独立函数是为了让这个守卫可被单测——它是「有意 teardown」与「假崩溃
/// toast」之间唯一的东西。
pub(super) fn watcher_owns_state(state_pid: Option<u32>, watched_pid: u32) -> bool {
    state_pid == Some(watched_pid)
}

/// 等待子进程退出；更新状态并发 Tauri 事件。持监管者 mutex 的独立 Arc 克隆，
/// 故该任务能安然度过中间的多次重启。
#[cfg(feature = "gui")]
pub(super) async fn watch_exit(inner: Arc<Mutex<DaemonState>>, app_handle: AppHandle, pid: u32) {
    // 在锁内把 Child **取出来**，然后在 await `wait()` 前丢开 guard。跨着
    // `child.wait().await` 持 `inner.lock().await` 会把 `mcp_daemon_restart` 永久
    // 死锁——`restart()` 的 `self.inner.lock().await` 会永远排在这个监视器后面，
    // 而那边明明坐着一个健康的 daemon。取出的写法让 guard 在块末即释放，`wait()`
    // 无锁，restart / shutdown / snapshot 才能继续。
    let mut child = {
        let mut g = inner.lock().await;
        match g.child.take() {
            Some(c) => c,
            None => {
                // 监管者已在我们脚下把子进程 teardown 掉（例如调了 shutdown()）；
                // 无事可做。
                return;
            }
        }
    };
    let exit_status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "mcp_daemon", "wait() failed: {e}");
            return;
        }
    };

    let exit_code = exit_status.code();
    let message = if let Some(code) = exit_code {
        format!("daemon exited with code {code}")
    } else {
        "daemon terminated by signal".to_string()
    };

    let payload = DaemonCrashedPayload {
        status: "crashed".into(),
        exit_code,
        message: message.clone(),
        last_error: None,
    };

    {
        let mut g = inner.lock().await;
        // 仅当本监视器观察的子进程 PID 仍等于当前监管者 PID 时才翻状态。重启会同时换掉
        // 两者；又因为 `teardown` 先清 `pid` 再 kill，连被有意 teardown 唤醒的监视器
        // 在此也只会看到 `None` 而保持安静。
        if watcher_owns_state(g.pid, pid) {
            g.status = DaemonStatus::Crashed { exit_code };
            g.last_error = Some(message.clone());
            g.pid = None;
            g.port = None;
            g.port_record = None;
        }
    }
    // 尽力发事件——即使事件总线因故已关闭，UI 也会在下一次 mcp_status 轮询时补上。
    if let Err(e) = app_handle.emit("daemon://crashed", &payload) {
        eprintln!("[daemon] emit daemon://crashed failed: {e}");
    }
}

/// [`watch_exit`] 的 web 形态对应物。同样的「锁内取走 Child、丢开 guard、
/// 再 await `wait()`」；唯一差别是没有 `AppHandle` 事件——web 形态没有 Tauri
/// 总线，`/api/mcp/status` 轮询覆盖同一片表面。
pub(super) async fn watch_exit_no_app(inner: Arc<Mutex<DaemonState>>, pid: u32) {
    let mut child = {
        let mut g = inner.lock().await;
        match g.child.take() {
            Some(c) => c,
            None => return,
        }
    };
    let exit_status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "mcp_daemon", "wait() failed: {e}");
            return;
        }
    };
    let exit_code = exit_status.code();
    let message = if let Some(code) = exit_code {
        format!("daemon exited with code {code}")
    } else {
        "daemon terminated by signal".to_string()
    };
    {
        let mut g = inner.lock().await;
        if watcher_owns_state(g.pid, pid) {
            g.status = DaemonStatus::Crashed { exit_code };
            g.last_error = Some(message);
            g.pid = None;
            g.port = None;
            g.port_record = None;
        }
    }
}
