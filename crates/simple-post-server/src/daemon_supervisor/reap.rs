//! 终止子进程这条侧：杀哪些 PID、走哪条回收通道、等多久算没走干净，以及调用它的
//! 停止侧动词。与 spawn 侧（父模块）分开，因为两者的失败语义相反——spawn 要留状态
//! 供 UI 呈现，teardown 要在固定预算内把状态清干净。

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::time::sleep;

use super::status::DaemonStatus;
use super::McpDaemonHandle;
use crate::port_registry;

/// teardown 在释放端口文件、放行替换进程之前，等被杀的 daemon 真正离开进程表
/// 的上限时长。刻意有界——卡死的进程不能把 UI 的启动/停止按钮永久吊住。
pub(super) const TEARDOWN_TIMEOUT: Duration = Duration::from_millis(2_000);
/// [`TEARDOWN_TIMEOUT`] 内部的轮询节奏。
pub(super) const TEARDOWN_POLL: Duration = Duration::from_millis(50);

/// 生命周期动词必须终止的一个 PID，附其可用的回收方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeardownTarget {
    pub pid: u32,
    /// `true` 表示监管者对该 PID 持有 `Child` 句柄——是我们 spawn 的，reap
    /// 归我们，可用 `try_wait()`。
    /// `false` 表示只有端口记录点名的*被收养* daemon：没有句柄，只能经
    /// [`port_registry::terminate_pid`] 带外杀掉。
    pub via_handle: bool,
}

/// 纯函数：teardown 必须终止哪些 PID。
///
/// 两个来源，各一个 PID：
///   * `owned_pid`——我们 spawn 且仍持有句柄的子进程。
///   * `adopted_pid`——端口记录点名、自己从未 spawn 的活 daemon。复用路径会填
///     `state.pid` 却不填 `state.child`，只查句柄的 teardown 会扑空，于是 `restart`
///     立刻重新收养了它本该替换掉的同一个进程，同时报告成功。
///
/// 同一 PID 至多出现一次（既 spawn 又收养的 daemon 是同一个进程，而句柄是更
/// 廉价的回收方式）。`0` 在这里滤掉而不是在每个调用点：spawn 后
/// `child.id()` 合法地返回 `None`，漏网的 `0` 会一路走到
/// `taskkill /F /PID 0`。
pub(super) fn teardown_targets(
    owned_pid: Option<u32>,
    adopted_pid: Option<u32>,
) -> Vec<TeardownTarget> {
    let mut out = Vec::new();
    if let Some(pid) = owned_pid.filter(|p| *p != 0) {
        out.push(TeardownTarget {
            pid,
            via_handle: true,
        });
    }
    if let Some(pid) = adopted_pid.filter(|p| *p != 0 && Some(*p) != owned_pid) {
        out.push(TeardownTarget {
            pid,
            via_handle: false,
        });
    }
    out
}

/// 对自有子进程做有界等待，等它被 reap。返回 `true` 表示 `try_wait()` 在预算内
/// 观察到退出。两种 host 形态下都很廉价——除等待本身外无系统调用——这也是句柄
/// 路径轮询比 PID 路径密得多的原因。
pub(super) async fn wait_for_child_exit(
    child: &mut tokio::process::Child,
    budget: Duration,
) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "try_wait during teardown failed: {e}");
                return false;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(TEARDOWN_POLL).await;
    }
}

/// 对无句柄的 PID 做有界等待，等它离开进程表。比 [`wait_for_child_exit`] 粗，
/// 因为 [`port_registry::is_pid_alive`] 在 Windows 上要 spawn 一个进程
/// （`tasklist`），按 50ms 轮询会让它主导整个 teardown。
pub(super) async fn wait_for_pid_exit(pid: u32, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if !port_registry::is_pid_alive(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(TEARDOWN_POLL).await;
    }
}

impl McpDaemonHandle {
    /// 手动 OFF 开关。既终止我们 spawn 的 daemon，也终止只是收养来的那个，释放端口
    /// 文件，并把状态留在 `Stopped` 且 `pid` / `port` 清空——于是下一次 `start`
    /// 干净地走全新 spawn 路径。什么都没在跑时调用也安全：每步 teardown 都是 no-op。
    pub async fn stop(&self) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Stopped).await?;
        Ok(self.snapshot())
    }

    /// `serve-web` 形态的 [`Self::restart`]：无 `AppHandle`，自带数据目录。与其 Tauri
    /// 对应物一样是共享路径的绑定，所以 spawn / teardown 的修复不会只落在一个动词
    /// 上而漏掉另一个。
    pub async fn restart_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Restarting).await?;
        let _ = self
            .ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish();
        Ok(self.snapshot())
    }

    /// 终止当前跟踪的 daemon，然后清空跟踪状态并删掉端口文件。由 `stop`、`restart`
    /// 与 `shutdown` 共用；`next` 是 teardown 进行期间调用方希望快照报告的状态
    /// （手动开关与应用退出用 `Stopped`，重启用 `Restarting`）。
    ///
    /// 顺序是硬要求：
    ///
    ///   1. **先**在锁外解析出要杀的 PID。存活探测在 Windows 上要 spawn 一个进程
    ///      （`tasklist`），跨着监管者 mutex 跑会把每次状态轮询都堵住。
    ///   2. 锁内：先设新状态，再清 `pid` / `port` / `port_record`，并取走子进程
    ///      句柄。先清正是 `watcher_owns_state` 守卫成立的原因：被我们自己那次
    ///      kill 唤醒的退出监视器此后匹配不上，也就无法用 `Crashed` 覆盖此状态。
    ///   3. 先 kill，再**有界等待**进程真正消失，然后才释放端口文件。固定睡
    ///      200ms 是靠猜：Windows 上子进程可能仍活着，而未关闭完的 socket 在别的
    ///      平台上会让端口留在 bind 队列外，替换进程抢不到端口，起来就是
    ///      `Unhealthy`。
    pub(super) async fn teardown(&self, next: DaemonStatus) -> Result<(), crate::error::AppError> {
        // (1) 锁外解析目标。端口记录是此处的权威：即便监管者从未 spawn 过该
        // daemon（`serve-web` 形态、连续两次重启、新 GUI 收养上一个启动的 daemon）
        // 它也仍在。
        let record_pid = port_registry::read()
            .filter(|rec| port_registry::is_pid_alive(rec.pid))
            .map(|rec| rec.pid);
        let (child, targets) = {
            let mut g = self.inner.lock().await;
            let owned_pid = g.child.as_ref().and_then(|c| c.id());
            let targets = teardown_targets(owned_pid, record_pid);
            g.status = next;
            g.pid = None;
            g.port = None;
            g.port_record = None;
            (g.child.take(), targets)
        };

        // (3) 终止。自有子进程先杀：我们持有句柄，`try_wait` 无需带外信号即可 reap。
        if let Some(mut child) = child {
            let _ = child.start_kill();
            if !wait_for_child_exit(&mut child, TEARDOWN_TIMEOUT).await {
                tracing::warn!(
                    target: "mcp_daemon",
                    "daemon pid {} still alive after {}ms — continuing anyway",
                    child.id().unwrap_or(0),
                    TEARDOWN_TIMEOUT.as_millis()
                );
            }
        }
        for target in targets.iter().filter(|t| !t.via_handle) {
            if !port_registry::terminate_pid(target.pid) {
                // 已经没了，或 OS 拒绝了。反正下面的等待才是真答案，故不让该动词失败。
                tracing::debug!(
                    target: "mcp_daemon",
                    "taskkill/kill -9 declined for adopted pid {}",
                    target.pid
                );
                continue;
            }
            if !wait_for_pid_exit(target.pid, TEARDOWN_TIMEOUT).await {
                tracing::warn!(
                    target: "mcp_daemon",
                    "adopted daemon pid {} still alive after {}ms — continuing anyway",
                    target.pid,
                    TEARDOWN_TIMEOUT.as_millis()
                );
            }
        }
        port_registry::release()
    }

    /// 尽力而为的关闭。由 GUI 退出钩子（及测试）调用。与手动停止开关共用同一条
    /// 生命周期 teardown，故应用退出会终止被*收养*的 daemon，而不是任其带着还在的
    /// 端口文件变孤儿。
    pub async fn shutdown(&self) -> Result<(), crate::error::AppError> {
        self.teardown(DaemonStatus::Stopped).await
    }
}
