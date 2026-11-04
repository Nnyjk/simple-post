//! HTTP MCP daemon 子进程的监管者。
//!
//! Tauri GUI 把同一个二进制以 `mcp` 子命令模式拉成子进程，通过 [`McpDaemonHandle`]
//! 暴露其生命周期，并在子进程意外退出时向 renderer 发 `daemon://crashed` 事件。
//!
//! 生命周期：`ensure_started` 收养端口文件指向的活 daemon（配置指纹一致时），否则拉起新
//! 子进程；`forward_stderr` 逐行转发子进程 stderr；`watch_exit` 在非零退出时发
//! `daemon://crashed`；`stop`（手动 OFF）与 `shutdown`（退出钩子）走同一条 `teardown`，
//! 它也终止本监管者只是*收养* 的 daemon（见 `reap::teardown_targets`）；`start` 是手动 ON
//! 开关且幂等，健康且被跟踪的 daemon 原样返回。
//!
//! `auto_start`（持久化偏好）决定 daemon 启动时是否自动起来，`start` / `stop`（运行时动词）
//! 决定它*现在*是否在跑；关掉 `auto_start` 不会杀掉已在运行的 daemon。
//!
//! 文件分工（本文件是门面，只持有 spawn 侧与三个 Tauri 绑定）：`status` 生命周期状态与快照
//! 投影、`state` 每次 spawn 的内部记账、`listen_config` 监听配置行 → 子进程 argv、
//! `probe` healthz 探测、`reap` 终止侧、`watcher` 退出监视器、`stderr` 日志转发。
//!
//! `ensure_started` 与 `ensure_started_with` 必须留在本文件：源码级回归测试按文本扫描它们，
//! 见 `daemon_supervisor_test.rs`。

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

// 只有 GUI host 会发 `daemon://crashed`。此 feature 之下本 crate 完全不依赖 Tauri，故容器镜像
// 免掉 GTK/WebKit、`--lib` 测试二进制免掉 Windows GUI runtime。发事件的一侧在 `watcher`，
// 本文件只接收句柄。
#[cfg(feature = "gui")]
use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::port_registry::{self, PortRecord};

mod listen_config;
mod log_buffer;
mod probe;
mod reap;
mod state;
mod status;
mod stderr;
mod watcher;

use listen_config::daemon_cli_args;
use log_buffer::LogBuffer;
use state::DaemonState;
use stderr::forward_stderr;
use watcher::watch_exit_no_app;
// 线格式类型是本模块的公开接口；其余子模块的东西一律只经 `pub(super)` 可见。
pub use status::{DaemonCrashedPayload, DaemonStatus};
// 保持 `crate::daemon_supervisor::TeardownTarget` 这条 crate 内路径不变。生产的 lib 构建里没有别的
// 引用方，故显式压掉未用告警——它是被测试用的导出面。
#[allow(unused_imports)]
pub(crate) use reap::TeardownTarget;
#[cfg(feature = "gui")]
use watcher::watch_exit;

/// Tauri 形态使用的主机侧数据目录——它没有数据目录参数可传。须与 daemon 子进程
/// 自行推导出的结果一致；`serve-web` 形态显式传自己的。
#[cfg(feature = "gui")]
fn host_data_dir() -> String {
    dirs::data_dir()
        .map(|p| p.join("simple-post").display().to_string())
        .unwrap_or_default()
}

/// 新拉起的子进程在共享 spawn 路径返回后还需要 host 提供的东西。
struct PendingWatch {
    inner: Arc<Mutex<DaemonState>>,
    pid: u32,
}

/// [`McpDaemonHandle::ensure_started_with`] 的返回值——唯一的主机无关 spawn 路径。
///
/// spawn 路径必须 100% 不含 Tauri，`AppHandle` 只能出现在这层薄薄的 `finish` 对里，且只从三个
/// Tauri 绑定（`ensure_started`、`start`、`restart`）可达。**不要把 `AppHandle` 移回
/// `ensure_started_with` 能看见的任何类型或参数。** 共享代码持有 `AppHandle` 会把 Tauri Windows
/// runtime 留在本模块的可达图里，`--lib` 测试二进制会在**加载**时死于
/// `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`（未解析的入口点是 `comctl32.dll!TaskDialogIndirect`，
/// 它只存在于 comctl32 **v6**），一个测试都跑不了。
/// 回归网：`daemon_supervisor_test.rs::the_shared_spawn_path_stays_free_of_tauri`。
struct Started {
    port: u16,
    /// 仅在刚拉起新子进程、仍需退出监视器时为 `Some`；复用路径为 `None`
    /// （没有新进程，也就没有东西可监视）。
    watch: Option<PendingWatch>,
}

impl Started {
    /// Tauri 形态：装上发 `daemon://crashed` 的监视器。
    #[cfg(feature = "gui")]
    fn tauri_finish(self, app_handle: &AppHandle) -> u16 {
        let port = self.port;
        if let Some(w) = self.watch {
            let app = app_handle.clone();
            tokio::spawn(async move {
                watch_exit(w.inner, app, w.pid).await;
            });
        }
        port
    }

    /// `serve-web` 形态：同一个监视器，只是不发 Tauri 事件——web UI 从
    /// `/api/mcp/status` 轮询得知 daemon 已死。
    fn poll_only_finish(self) -> u16 {
        let port = self.port;
        if let Some(w) = self.watch {
            tokio::spawn(async move {
                watch_exit_no_app(w.inner, w.pid).await;
            });
        }
        port
    }
}

/// 放在 Tauri state 里的公开句柄。克隆廉价（近似 Arc），可随意拷进 spawn 的任务。
#[derive(Clone)]
pub struct McpDaemonHandle {
    inner: Arc<Mutex<DaemonState>>,
    /// spawn 时读 `mcp.http_config` 用的连接池，好让子进程 CLI 与设置面板一致。裸 `new()` 为
    /// `None`，此时回退到 [`crate::domain::McpHttpConfig::default`]。
    /// 两种真实 host 形态都走 [`Self::with_pool`]：在 *spawn* 时读该行而非构造时，用户改完设置
    /// 再点重启就能生效，不必重建受管 state。
    pool: Option<crate::db::DbPool>,
    /// daemon 的日志环形缓冲。**刻意放在状态锁之外**：读日志不必抢那把锁，不会被「监管者正忙」
    /// 挡住；缓冲自己的互斥量只护住那几百行字符串，临界区是微秒级。
    logs: Arc<LogBuffer>,
}

impl Default for McpDaemonHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl McpDaemonHandle {
    /// 不碰数据库的句柄——spawn 时回退到默认监听配置。供测试与任何不带数据库驱动
    /// 监管者的嵌入方使用。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
            pool: None,
            logs: Arc::new(LogBuffer::new()),
        }
    }

    /// 两种真实 host 形态共用的构造器。
    pub fn with_pool(pool: crate::db::DbPool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
            pool: Some(pool),
            logs: Arc::new(LogBuffer::new()),
        }
    }

    /// daemon 的日志 tail。廉价——复制一份缓冲里的字符串列表。
    pub fn log_tail(&self) -> crate::dto::McpLogTailDto {
        let tail = self.logs.snapshot();
        crate::dto::McpLogTailDto {
            lines: tail.lines,
            started_at_ms: tail.started_at_ms,
            truncated: tail.truncated,
            capturing: tail.capturing,
        }
    }

    /// 幂等启动（Tauri 形态）。[`Self::ensure_started_with`] 的薄绑定：GUI 的
    /// `.setup()` 与 `mcp_auto_start_set` 调的就是这个签名。
    #[cfg(feature = "gui")]
    pub async fn ensure_started(
        &self,
        app_handle: &AppHandle,
    ) -> Result<u16, crate::error::AppError> {
        Ok(self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle))
    }

    /// 所有 host 形态共用的唯一 spawn 路径。
    ///
    /// 刻意不含 `AppHandle`（及任何其他 Tauri 东西），好让 `--lib` 测试二进制
    /// 不链入 GUI runtime——改签名前先读 [`Started`] 上的长注记。
    ///
    ///   1. `port_registry::read_entry()` 返回的记录若 PID 仍活着**且**
    ///      `/mcp/healthz` 有响应 → 看配置指纹：指纹一致就**收养**那个 daemon
    ///      （daemon 没清理端口文件的场景），指纹不一致或读不出指纹就重启它。
    ///      被收养的 daemon 记 `state.pid` 但 `state.child` 保持 `None`，因为我们从未
    ///      spawn 它、没有 reap 句柄——这对 stop/restart 意味着什么见
    ///      `reap::teardown_targets`。也不返回监视器：没有子进程句柄可供观察。
    ///   2. 否则释放陈旧文件、申领新端口、spawn 进程、以新 PID 写端口记录（连同这份
    ///      daemon 实际在服务的配置的指纹），并等最多 5s 直到 `/mcp/healthz` 转绿。
    ///   3. 在此启动 stderr 转发器，并返回 `(state, pid)`，由*调用方*据此建自己的
    ///      退出监视器。该任务活得比本函数久，直到子进程死亡为止更新状态。
    async fn ensure_started_with(&self, data_dir: &str) -> Result<Started, crate::error::AppError> {
        // 一次解析、提前取好：它同时喂给子进程命令行、端口记录里的配置指纹与
        // 「能不能收养」的判断，三者必须是同一份配置——记录若声称 `127.0.0.1` 而
        // daemon 实际绑在 `0.0.0.0`，`requires_restart` 会在每次保存时都回答
        // 「需要重启」。
        let http_config = self.http_config();
        let wanted_fingerprint = crate::http_config::config_fingerprint(&http_config);

        // ----- 复用路径 ------------------------------------------------
        if let Some(entry) = port_registry::read_entry() {
            if port_registry::is_pid_alive(entry.record.pid)
                && port_registry::healthz(entry.record.port).await
            {
                match reuse_decision(entry.config_fingerprint.as_deref(), &wanted_fingerprint) {
                    ReuseDecision::Adopt => return Ok(self.adopt(entry.record).await),
                    ReuseDecision::Restart => {
                        // 那个 daemon 服务的不是我要的这份配置（或旧记录里没有指纹，
                        // 判断不了）：杀掉它，让下面的 spawn 路径拉一个新的。
                        self.teardown(DaemonStatus::Restarting).await?;
                    }
                }
            }
        }

        // ----- 全新 spawn 路径 ------------------------------------------
        let mut g = self.inner.lock().await;
        // 持锁复查：等锁期间别的任务可能刚 spawn 过。
        let mut restart_first = false;
        if let Some(entry) = port_registry::read_entry() {
            if port_registry::is_pid_alive(entry.record.pid)
                && port_registry::healthz(entry.record.port).await
            {
                match reuse_decision(entry.config_fingerprint.as_deref(), &wanted_fingerprint) {
                    ReuseDecision::Adopt => {
                        // 收养要拿同一把锁：先放锁。
                        drop(g);
                        return Ok(self.adopt(entry.record).await);
                    }
                    ReuseDecision::Restart => restart_first = true,
                }
            }
        }
        if restart_first {
            // 重启要杀掉那个 daemon，而 teardown 也要拿这把锁：先放锁。
            drop(g);
            self.teardown(DaemonStatus::Restarting).await?;
            g = self.inner.lock().await;
        }
        // 状态置 Starting。下面任何一步失败都会把它翻成 Crashed / Unhealthy 供 UI 呈现。
        g.status = DaemonStatus::Starting;
        g.last_error = None;
        drop(g); // 在 await spawn 前释放（tokio::process::Command 需要 &mut self）

        // 先清陈旧文件再挑新端口，免得 port_registry::write() 里的读路径撞上它。
        let _ = port_registry::release();

        // ----- 分配端口 -------------------------------------------
        // 绑 0 → 由 OS 选。真实端口从 local_addr() 出来，随即 drop 掉 listener
        // 让 daemon 去抢（Linux 上无竞态，因为我们持锁且只有一个 GUI 会申领；
        // 真 TOCTOU 需要两个 GUI 恰好同时启动）。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(crate::error::AppError::Io)?;
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .map_err(crate::error::AppError::Io)?;
        drop(listener);

        // ----- spawn 子进程 -------------------------------------------
        let exe = std::env::current_exe().map_err(crate::error::AppError::Io)?;
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(daemon_cli_args(port, &http_config))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true); // GUI 被 drop ⇒ 子进程收到 SIGKILL

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut g = self.inner.lock().await;
                g.status = DaemonStatus::Crashed { exit_code: None };
                g.last_error = Some(format!("spawn failed: {e}"));
                return Err(crate::error::AppError::Io(e));
            }
        };
        let pid = child.id().unwrap_or(0);
        let stderr = child.stderr.take();

        let rec = PortRecord::new(pid, port, &http_config.host, data_dir.to_string());
        // 端口记录里带上这份 daemon 实际在服务的配置的指纹：下一个监管者（桌面端或
        // serve-web）读到它就能判断「这个 daemon 服务的是不是我要的那份配置」。
        if let Err(e) = port_registry::write_with_fingerprint(&rec, Some(&wanted_fingerprint)) {
            // 端口文件写失败——子进程归我们，返回前先 reap，别留下一个没有端口文件的
            // daemon。
            let _ = child.kill().await;
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Crashed { exit_code: None };
            g.last_error = Some(format!("port registry: {e:?}"));
            return Err(e);
        }

        // ----- 更新公开状态 -------------------------------------------
        {
            let mut g = self.inner.lock().await;
            g.pid = Some(pid);
            g.port = Some(port);
            g.port_record = Some(rec);
            g.status = DaemonStatus::Starting;
            g.started_at = Some(Instant::now());
        }
        // 每次 spawn 换一份日志：重启后的日志不该与上一轮混在一起。
        self.logs.start_capture();

        // ----- 等 healthz --------------------------------------------
        let mut healthy = false;
        for _ in 0..50 {
            // 50 × 100ms = 5s
            if port_registry::healthz(port).await {
                healthy = true;
                break;
            }
            // 子进程若已退出就快速退出——没必要为一个立刻死掉的进程烧完 5s。
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        if !healthy {
            // 把子进程留在 `state` 里而不是交给监视器：监视器若*拿走*句柄就会剥掉 `state.child`，
            // 让 `stop` / `restart` 无法 reap 这个进程。留在这里，不健康启动时
            // `teardown_targets` 的 `owned_pid` 通道依然有效。
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Unhealthy;
            g.last_error = Some("healthz never came up within 5s".into());
            g.child = Some(child);
            return Err(crate::error::AppError::Validation(
                "mcp daemon healthz timeout".into(),
            ));
        }

        // ----- 交给后台任务 -------------------------------------------
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr(stderr, Arc::clone(&self.logs)));
        }
        {
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Running;
            g.child = Some(child);
        }

        // 监视器归调用方去装：这是本路径唯一拒绝知晓的主机行为。
        Ok(Started {
            port,
            watch: Some(PendingWatch {
                inner: self.inner.clone(),
                pid,
            }),
        })
    }

    /// 收养一个已经在服务的 daemon：记下它的身份，把状态置为运行中。
    /// 被收养的 daemon 是外部启动的：我们从未 spawn 它，故没有 stderr 管道，界面上的 MCP
    /// 日志面板在这种情形下是空的。
    async fn adopt(&self, rec: PortRecord) -> Started {
        let port = rec.port;
        {
            let mut g = self.inner.lock().await;
            g.pid = Some(rec.pid);
            g.port = Some(rec.port);
            g.port_record = Some(rec);
            g.status = DaemonStatus::Running;
            g.started_at = Some(Instant::now());
        }
        self.logs.stop_capture();
        Started { port, watch: None }
    }

    /// 手动 ON 开关（Tauri 形态）。天生幂等：健康且被跟踪的 daemon 原样返回——不 kill、不重拉、
    /// 不折腾端口；只有确实停止 / 崩溃 / 不健康的 daemon 才走 spawn 路径。
    /// 一处例外：端口文件里那个 daemon 服务的配置与现在要用的这份不一致时它被换掉（判据见
    /// `reuse_decision`）；本监管者已经在跟踪一个健康的 daemon 时不走那条路。
    #[cfg(feature = "gui")]
    pub async fn start(
        &self,
        app_handle: &AppHandle,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        if self.is_serving().await {
            return Ok(self.snapshot());
        }
        // 刻意忽略返回的端口：调用方要的是整份快照，里面已带新的 `port`。
        let _ = self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle);
        Ok(self.snapshot())
    }

    /// `serve-web` 形态的 [`Self::start`]。
    pub async fn start_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish();
        Ok(self.snapshot())
    }

    /// 重启：彻底 teardown（见 [`Self::teardown`]）再拉起一个新的。返回重启后的
    /// `McpStatusDto`，调用方无需再轮询即可回显新 `pid` / `port`。
    #[cfg(feature = "gui")]
    pub async fn restart(
        &self,
        app_handle: &AppHandle,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Restarting).await?;
        let _ = self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle);
        Ok(self.snapshot())
    }
}

/// 端口文件里那个 daemon 与「我现在要的那份配置」的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReuseDecision {
    /// 指纹一致：那个 daemon 正在服务这份配置。收养它——不 kill、不重拉，进行中的
    /// MCP 会话不断。
    Adopt,
    /// 指纹不一致，或旧记录里没有指纹（判断不了）：那个 daemon 服务的不是这份配置
    /// （或无从判断），重启。
    Restart,
}

/// 端口文件里那个 daemon 能不能被收养？指纹一致 → [`ReuseDecision::Adopt`]；指纹不一致或读不出
/// 指纹 → [`ReuseDecision::Restart`]。
/// 读不出指纹就是「判断不了」，判断不了的方向是重启——与 `http_config::requires_restart` 对同一
/// 情形给的回答一致。
pub(crate) fn reuse_decision(recorded: Option<&str>, wanted: &str) -> ReuseDecision {
    match recorded {
        Some(recorded) if recorded == wanted => ReuseDecision::Adopt,
        _ => ReuseDecision::Restart,
    }
}

// -------------------------------------------------------------------------
// 测试
// -------------------------------------------------------------------------

/// 生命周期决策的纯逻辑测试（杀目标选择、启动幂等性、退出监视器守卫）放在同级的
/// 独立文件里，故它们绝不会碰到真实进程表或真实的端口文件。
#[cfg(test)]
#[path = "daemon_supervisor_test.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "daemon_supervisor_handle_test.rs"]
mod handle_tests;

#[cfg(test)]
#[path = "daemon_supervisor_reuse_test.rs"]
mod reuse_tests;
