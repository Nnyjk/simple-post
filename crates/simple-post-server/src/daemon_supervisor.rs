//! HTTP MCP daemon 子进程的监管者。
//!
//! Tauri GUI 把同一个二进制以 `mcp` 子命令模式拉成子进程，通过 [`McpDaemonHandle`]
//! 暴露其生命周期，并在子进程意外退出时向 renderer 发 `daemon://crashed` 事件。
//!
//! 生命周期：
//!   * `ensure_started` 由 GUI 的 `.setup()` 调用。若端口文件指向一个活着的 PID，
//!     直接复用（重启笔记本后的廉价路径），否则在 OS 分配的端口上拉起子进程。
//!   * `forward_stderr_and_parse_metrics` 逐行读子进程 stderr，把指标行喂进内存里的
//!     `McpMetrics`，`mcp_status` 由此反映实时数字。
//!   * `watch_exit` 等待子进程，非零退出时向 Tauri app handle 发
//!     `daemon://crashed`；renderer 弹 toast 并让用户重试。
//!   * `stop` 是手动 OFF 开关（Tauri `mcp_daemon_stop` / `POST /api/mcp/stop`），
//!     `shutdown` 是退出钩子。两者走同一条 `teardown`，它也会终止本监管者只是
//!     *收养* 的 daemon，见 [`teardown_targets`]。
//!   * `start` 是手动 ON 开关且幂等：健康且被跟踪的 daemon 原样返回，不重新拉起。
//!
//! `auto_start`（持久化偏好）与 `start` / `stop`（运行时动词）是叠加而非重叠的
//! 关系：偏好决定 daemon 启动时是否自动起来，开关决定它*现在*是否在跑。
//! 关掉 `auto_start` 不会杀掉已在运行的 daemon。

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
// 只有 GUI host 会发 `daemon://crashed`。此 feature 之下本 crate 完全不依赖
// Tauri——这正是容器镜像免掉 GTK/WebKit、`--lib` 测试二进制免掉 Windows GUI
// runtime 的原因。
#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::domain::McpHttpConfig;
use crate::mcp_metrics::{parse_metric_line, McpMetrics};
use crate::port_registry::{self, PortRecord};

/// renderer 关心的生命周期状态。`#[serde(rename_all = "snake_case")]` 固定线格式，
/// React 侧可直接按字符串分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatus {
    /// 尚未 spawn 过任何进程（初始态）。
    Stopped,
    /// 已发出 spawn，但 healthz 还没回来。
    Starting,
    /// healthz 绿，子进程 PID 仍存活。
    Running,
    /// 用户触发重启，旧子进程已杀，新进程正在拉起。
    Restarting,
    /// 子进程非零退出（或被信号杀死）。保留 `exit_code` 供 renderer 展示。
    Crashed { exit_code: Option<i32> },
    /// 子进程活着但 `/mcp/healthz` 失败——通常是首次请求卡死或死锁。不自动重启，
    /// 须由用户处理。
    Unhealthy,
}

impl DaemonStatus {
    fn as_str(self) -> &'static str {
        match self {
            DaemonStatus::Stopped => "stopped",
            DaemonStatus::Starting => "starting",
            DaemonStatus::Running => "running",
            DaemonStatus::Restarting => "restarting",
            DaemonStatus::Crashed { .. } => "crashed",
            DaemonStatus::Unhealthy => "unhealthy",
        }
    }
}

/// `daemon://crashed` 的事件体。renderer 用 `message` 弹一个带「重试」动作的
/// toast，该动作调用 `restart_daemon`（Tauri 命令，见 `commands/mcp_status.rs`）。
#[derive(Debug, Clone, Serialize)]
pub struct DaemonCrashedPayload {
    pub status: String, // 恒为 "crashed"——与 DTO 对称
    pub exit_code: Option<i32>,
    pub message: String,
    pub last_error: Option<String>,
}

/// 内部：每次 spawn 的记账。放在 tokio mutex 后，公开方法才能是 `&self`
/// （Tauri state 需要共享引用）。
struct DaemonState {
    pid: Option<u32>,
    port: Option<u16>,
    port_record: Option<PortRecord>,
    /// 保留句柄，好让 `shutdown` 与 `restart` 能 `kill()` 而不丢它。某些平台上
    /// 子进程已被别处 reap 后再 `wait()` 会报错，所以 `child` 一直留到
    /// `shutdown`，reap 归我们。
    child: Option<tokio::process::Child>,
    metrics: Arc<McpMetrics>,
    status: DaemonStatus,
    started_at: Option<Instant>,
    last_error: Option<String>,
}

impl DaemonState {
    fn empty() -> Self {
        Self {
            pid: None,
            port: None,
            port_record: None,
            child: None,
            metrics: Arc::new(McpMetrics::new()),
            status: DaemonStatus::Stopped,
            started_at: None,
            last_error: None,
        }
    }
}
/// 构造守护进程的真实 CLI 参数，桌面端与 Web 端共享同一份契约。
///
/// 除端口外全部取自持久化的 `mcp.http_config` 行：在这里读该行，设置面板与子
/// 进程命令行就不可能各说各话。
///
/// `port` 是监管者刚申领到的 OS 分配端口，显式传入而非让子进程自己再挑一次——
/// 监管者会把这个端口写进 `mcp.port` 并拿它做 health 探测。子进程自身的
/// `mcp.http_config.port` 只对手工运行的 `simple-post mcp` 起作用。
///
/// 空的 allowlist 以 `--allowed-hosts=` / `--allowed-origins=` 传下去，clap 解析成
/// 空列表。这是有意义的而非凑数：配合 `--allow-remote`，空 host 列表意味着
/// **拒绝**所有非 loopback 的 `Host`；省掉该 flag 则要依赖默认值恰好一致。
fn daemon_cli_args(port: u16, config: &McpHttpConfig) -> Vec<String> {
    let mut args = vec![
        "mcp".to_string(),
        "--transport=http".to_string(),
        format!("--port={port}"),
        format!("--bind={}", config.host),
        format!("--path={}", config.normalized_path()),
    ];
    if config.allow_remote {
        args.push("--allow-remote".to_string());
    }
    args.push(format!(
        "--allowed-hosts={}",
        config.allowed_hosts.join(",")
    ));
    args.push(format!(
        "--allowed-origins={}",
        config.allowed_origins.join(",")
    ));
    args
}

/// teardown 在释放端口文件、放行替换进程之前，等被杀的 daemon 真正离开进程表
/// 的上限时长。刻意有界——卡死的进程不能把 UI 的启动/停止按钮永久吊住。
const TEARDOWN_TIMEOUT: Duration = Duration::from_millis(2_000);
/// [`TEARDOWN_TIMEOUT`] 内部的轮询节奏。
const TEARDOWN_POLL: Duration = Duration::from_millis(50);

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
/// # 为何 Tauri 绑定是*本类型上的方法*而非 spawn 路径的参数
///
/// spawn 路径必须 100% 不含 Tauri，`AppHandle` 只能出现在下面这层薄薄的
/// `finish` 对里，且只从三个 Tauri 绑定（`ensure_started`、`start`、`restart`）
/// 可达。**不要把 `AppHandle` 移回 `ensure_started_with` 能看见的任何类型或
/// 参数。** 一个被 `tokio::spawn` 的共享代码持有的 `AppHandle` 会把整个 Tauri
/// Windows runtime 留在这个模块的可达图里：`--lib` 测试二进制会因此链上
/// `tao` / `muda` 的五个 DLL 与 +2.4 MB GUI/COM/shell 代码，并在**加载**时死于
/// `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`（未解析的入口点是
/// `comctl32.dll!TaskDialogIndirect`，它只存在于 comctl32 **v6**——经 activation
/// context 解析的 side-by-side 程序集，而非 `System32` 里的 v5 副本），一个测试都
/// 跑不了。
///
/// `daemon_supervisor_test.rs::the_shared_spawn_path_stays_free_of_tauri` 是这
/// 条不变量的回归网。
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
fn teardown_targets(owned_pid: Option<u32>, adopted_pid: Option<u32>) -> Vec<TeardownTarget> {
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

/// 纯函数：一次「启动」请求是否已被满足。
///
/// 手动 ON 开关的幂等规则：若监管者跟踪的 daemon 仍能答 healthz，就返回该
/// daemon 的身份而不 kill 重拉。`tracked_port.is_some()` 必须与状态同时成立——
/// `Running` 但无跟踪端口是「已收养、随后丢了端口记录」的状态，此处无法确认存活，
/// 而重新 spawn 才是唯一诚实的答案。
// 纯决策函数且不含 Tauri，故始终参与编译——`daemon_supervisor_test.rs` 直接测它。
// 它唯一的生产调用方（`is_serving`）受 GUI feature 门控，故在无 GUI 的 feature 上
// 挂 `allow(dead_code)`。
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
fn start_is_noop(status: DaemonStatus, tracked_port: Option<u16>, healthy: bool) -> bool {
    status == DaemonStatus::Running && tracked_port.is_some() && healthy
}

/// 纯函数：这个退出监视器是否仍拥有监管者状态。
///
/// 两个 `watch_exit` 变体都只在自己监视的 daemon 仍是状态所指的那个时才把共享
/// 状态翻成 `Crashed`。`teardown` 会**先**清 `pid` 再 kill，所以被 teardown 自己
/// 那次 kill 唤醒的监视器看到 `None`，保持安静，不会覆盖动词刚设的
/// `Restarting` / `Stopped`，更不会覆盖后续 start 在同一 handle 上装的新状态。
/// 保持成独立函数是为了让这个守卫可被单测——它是「有意 teardown」与「假崩溃
/// toast」之间唯一的东西。
fn watcher_owns_state(state_pid: Option<u32>, watched_pid: u32) -> bool {
    state_pid == Some(watched_pid)
}

/// 对自有子进程做有界等待，等它被 reap。返回 `true` 表示 `try_wait()` 在预算内
/// 观察到退出。两种 host 形态下都很廉价——除等待本身外无系统调用——这也是句柄
/// 路径轮询比 PID 路径密得多的原因。
async fn wait_for_child_exit(child: &mut tokio::process::Child, budget: Duration) -> bool {
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
async fn wait_for_pid_exit(pid: u32, budget: Duration) -> bool {
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

/// 放在 Tauri state 里的公开句柄。克隆廉价（近似 Arc），可随意拷进 spawn 的任务。
#[derive(Clone)]
pub struct McpDaemonHandle {
    inner: Arc<Mutex<DaemonState>>,
    /// spawn 时读 `mcp.http_config` 用的连接池，好让子进程 CLI 与设置面板一致。
    ///
    /// 测试用的裸 `new()` 为 `None`，此时回退到 [`McpHttpConfig::default`]。两种
    /// 真实 host 形态（`lib.rs::run` 与 `web_api::serve`）都走
    /// [`Self::with_pool`] 构造——在 *spawn* 时读该行而非构造时，正是为了用户改完
    /// 设置再点重启就能生效，不必重建受管 state。
    ///
    /// `r2d2::Pool` 底层是 `Arc`，故此克隆廉价。
    pool: Option<crate::db::DbPool>,
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
        }
    }

    /// 两种真实 host 形态共用的构造器。
    pub fn with_pool(pool: crate::db::DbPool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
            pool: Some(pool),
        }
    }

    /// renderer 想要的当前状态快照。廉价——只是加锁加拷贝。返回
    /// `crate::dto::McpStatusDto`；字段若变，只需改这一处。
    pub fn snapshot(&self) -> crate::dto::McpStatusDto {
        // 用 `try_lock`：Tauri 命令在主线程同步调用，阻塞会与监管者自己的
        // 更新任务互锁。锁被占时直接报 "starting" 而不是等——UI 允许落后一帧。
        let state = match self.inner.try_lock() {
            Ok(s) => s,
            Err(_) => {
                // 锁被占：拿不到锁就取不到工具列表，故返回空列表，让下一次轮询
                // 拿到真值。
                return crate::dto::McpStatusDto {
                    status: DaemonStatus::Starting.as_str().to_string(),
                    running: false,
                    pid: None,
                    port: None,
                    uptime_ms: 0,
                    requests_handled: 0,
                    current_connections: 0,
                    registered_tools: Vec::new(),
                    last_error: Some("supervisor busy".into()),
                };
            }
        };
        let snap = state.metrics.snapshot();
        let status_str = state.status.as_str().to_string();
        // `running` 是 `status` 的派生视图，为仍读这个 bool 的调用方保留。仅严格
        // 的 `"running"` 计入——`starting` / `restarting` 是瞬态，renderer 对这两者
        // 应改用生命周期字符串分支。
        let running = status_str == "running";
        crate::dto::McpStatusDto {
            status: status_str,
            running,
            pid: state.pid,
            port: state.port,
            uptime_ms: snap.uptime_ms,
            requests_handled: snap.requests_handled,
            // `current_connections` 是为将来某条 stderr 指标行预留的；daemon 尚未发出
            // 这样的行，故硬编码 `0`。先把这个计数器放上线格式里是为了向前兼容——
            // daemon 真的开始发它时，UI 不必改 schema。
            current_connections: 0,
            registered_tools: crate::mcp::registered_tool_names(),
            last_error: state.last_error.clone(),
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

    /// `serve-web` 形态的 [`Self::ensure_started`]：无 `AppHandle`，自带数据目录。
    pub async fn ensure_started_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<u16, crate::error::AppError> {
        Ok(self
            .ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish())
    }

    /// 所有 host 形态共用的唯一 spawn 路径。
    ///
    /// 刻意不含 `AppHandle`（及任何其他 Tauri 东西），好让 `--lib` 测试二进制
    /// 不链入 GUI runtime——改签名前先读 [`Started`] 上的长注记。
    ///
    ///   1. `port_registry::read()` 返回的记录若 PID 仍活着**且** `/mcp/healthz`
    ///      有响应 → 复用（重启后 daemon 没清理端口文件的场景）。被复用的 daemon 是
    ///      *被收养*的：记 `state.pid` 但 `state.child` 保持 `None`，因为我们从未
    ///      spawn 它、没有 reap 句柄——这对 stop/restart 意味着什么见
    ///      [`teardown_targets`]。也不返回监视器：没有子进程句柄可供观察。
    ///   2. 否则释放陈旧文件、申领新端口、spawn 进程、以新 PID 写端口记录，并等
    ///      最多 5s 直到 `/mcp/healthz` 转绿。
    ///   3. 在此启动 stderr 转发器，并返回 `(state, pid)`，由*调用方*据此建自己的
    ///      退出监视器。该任务活得比本函数久，直到子进程死亡为止更新状态。
    async fn ensure_started_with(&self, data_dir: &str) -> Result<Started, crate::error::AppError> {
        // ----- 复用路径 ------------------------------------------------
        if let Some(rec) = port_registry::read() {
            if port_registry::is_pid_alive(rec.pid) && port_registry::healthz(rec.port).await {
                let mut g = self.inner.lock().await;
                g.pid = Some(rec.pid);
                g.port = Some(rec.port);
                g.port_record = Some(rec.clone());
                g.status = DaemonStatus::Running;
                g.started_at = Some(Instant::now());
                return Ok(Started {
                    port: rec.port,
                    watch: None,
                });
            }
        }

        // ----- 全新 spawn 路径 ------------------------------------------
        // 一次解析、提前取好：它同时喂给子进程命令行和端口记录，两者必须一致——
        // 记录若声称 `127.0.0.1` 而 daemon 实际绑在 `0.0.0.0`，`requires_restart`
        // 会在每次保存时都回答「需要重启」。
        let http_config = self.http_config();
        let mut g = self.inner.lock().await;
        // 持锁复查：等锁期间别的任务可能刚 spawn 过。
        if let Some(rec) = port_registry::read() {
            if port_registry::is_pid_alive(rec.pid) && port_registry::healthz(rec.port).await {
                g.pid = Some(rec.pid);
                g.port = Some(rec.port);
                g.port_record = Some(rec.clone());
                g.status = DaemonStatus::Running;
                g.started_at = Some(Instant::now());
                return Ok(Started {
                    port: rec.port,
                    watch: None,
                });
            }
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
        if let Err(e) = port_registry::write(&rec) {
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
            // 注意：保留此前的 `g.metrics`——它可能已被更早的一轮更新过，长驻运行中复用
            // 本 handle 重启时还要继续用。
        }

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
            // 把子进程留在 `state` 里而不是交给监视器：spawn 路径不知道 host 就装不了监视器
            // （这正是 [`Started`] 的全部要点），而监视器若*拿走*句柄就会剥掉
            // `state.child`，让 `stop` / `restart` 无法 reap 这个进程。留在这里，
            // 不健康启动时 [`teardown_targets`] 的 `owned_pid` 通道依然有效。
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Unhealthy;
            g.last_error = Some("healthz never came up within 5s".into());
            g.child = Some(child);
            return Err(crate::error::AppError::Validation(
                "mcp daemon healthz timeout".into(),
            ));
        }

        // ----- 交给后台任务 -------------------------------------------
        let metrics = {
            let g = self.inner.lock().await;
            Arc::clone(&g.metrics)
        };
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr_and_parse_metrics(stderr, metrics));
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

    /// 手动 ON 开关（Tauri 形态）。天生幂等：健康且被跟踪的 daemon 原样返回——不
    /// kill、不重拉、不折腾端口；只有确实停止 / 崩溃 / 不健康的 daemon 才走 spawn
    /// 路径。
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

    /// 手动 OFF 开关。既终止我们 spawn 的 daemon，也终止只是收养来的那个，释放端口
    /// 文件，并把状态留在 `Stopped` 且 `pid` / `port` 清空——于是下一次 `start`
    /// 干净地走全新 spawn 路径。什么都没在跑时调用也安全：每步 teardown 都是 no-op。
    pub async fn stop(&self) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Stopped).await?;
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

    /// 当前跟踪的 daemon 是否仍在服务？探测在锁外跑（它是一次 HTTP 往返），判断本身
    /// 是纯函数 [`start_is_noop`]。
    #[cfg(feature = "gui")]
    async fn is_serving(&self) -> bool {
        let (status, port) = {
            let g = self.inner.lock().await;
            (g.status, g.port)
        };
        let healthy = match port {
            Some(p) => port_registry::healthz(p).await,
            None => false,
        };
        start_is_noop(status, port, healthy)
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
    ///      句柄。先清正是 [`watcher_owns_state`] 守卫成立的原因：被我们自己那次
    ///      kill 唤醒的退出监视器此后匹配不上，也就无法用 `Crashed` 覆盖此状态。
    ///   3. 先 kill，再**有界等待**进程真正消失，然后才释放端口文件。固定睡
    ///      200ms 是靠猜：Windows 上子进程可能仍活着，而未关闭完的 socket 在别的
    ///      平台上会让端口留在 bind 队列外，替换进程抢不到端口，起来就是
    ///      `Unhealthy`。
    async fn teardown(&self, next: DaemonStatus) -> Result<(), crate::error::AppError> {
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

    /// 要 spawn 的监听配置：本 handle 带池时读持久化行，否则用出厂默认。
    ///
    /// 在 *spawn* 时读而非构造时，所以用户存了新设置再点重启就足够，无需重建受管
    /// state。
    ///
    /// **坏行 fail-closed**：`mcp.http_config` 读不出来就退回
    /// [`McpHttpConfig::default`]——只监听 loopback——而不是把错误抛出去拒绝启动。
    /// 手工编辑或截断的行不该有能力把 daemon 弄废，而默认值正是那个不暴露任何东西
    /// 的方向。
    fn http_config(&self) -> McpHttpConfig {
        let Some(pool) = self.pool.as_ref() else {
            return McpHttpConfig::default();
        };
        match crate::db::with_conn(pool, crate::db::preferences::mcp_http_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "mcp_daemon",
                    "mcp.http_config unreadable ({e:?}) — spawning on the loopback-only default"
                );
                McpHttpConfig::default()
            }
        }
    }
}

/// daemon stderr → 指标转发器。读到 EOF（进程退出）为止。
async fn forward_stderr_and_parse_metrics(
    reader: tokio::process::ChildStderr,
    metrics: Arc<McpMetrics>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Some(snap) = parse_metric_line(&line) {
                    metrics.merge_from_stderr(&snap);
                } else {
                    // 非指标行 presumed 是 daemon 的 tracing / 日志行，转投我们自己的
                    // tracing 通道，好让开发控制台同时看到两路流。
                    tracing::debug!(target: "mcp_daemon", "{}", line);
                }
            }
            Ok(None) => break, // EOF —— 子进程关闭了 stderr
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "stderr read: {e}");
                break;
            }
        }
    }
}

/// 等待子进程退出；更新状态并发 Tauri 事件。持监管者 mutex 的独立 Arc 克隆，
/// 故该任务能安然度过中间的多次重启。
#[cfg(feature = "gui")]
async fn watch_exit(inner: Arc<Mutex<DaemonState>>, app_handle: AppHandle, pid: u32) {
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

/// [`Self::watch_exit`] 的 web 形态对应物。同样的「锁内取走 Child、丢开 guard、
/// 再 await `wait()`」；唯一差别是没有 `AppHandle` 事件——web 形态没有 Tauri
/// 总线，`/api/mcp/status` 轮询覆盖同一片表面。
async fn watch_exit_no_app(inner: Arc<Mutex<DaemonState>>, pid: u32) {
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

// -------------------------------------------------------------------------
// 测试
// -------------------------------------------------------------------------

/// 生命周期决策的纯逻辑测试（杀目标选择、启动幂等性、退出监视器守卫）放在同级的
/// 独立文件里，故它们绝不会碰到真实进程表或真实的端口文件。
#[cfg(test)]
#[path = "daemon_supervisor_test.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_cli_args_use_real_http_transport_flag() {
        assert_eq!(
            daemon_cli_args(43123, &McpHttpConfig::default()),
            vec![
                "mcp",
                "--transport=http",
                "--port=43123",
                "--bind=127.0.0.1",
                "--path=/mcp",
                "--allowed-hosts=",
                "--allowed-origins=",
            ]
        );
    }

    #[test]
    fn daemon_cli_args_carry_the_persisted_listener_configuration() {
        // 子进程 CLI 必须与设置面板写入的那一行一致，否则面板显示的是 LAN daemon 而它
        // 实际仍绑 loopback（或反过来）。
        let cfg = McpHttpConfig {
            host: "0.0.0.0".into(),
            path: "/ai/mcp/".into(),
            allow_remote: true,
            allowed_hosts: vec!["studio.local".into(), "10.0.0.5".into()],
            allowed_origins: vec!["https://claude.ai".into()],
            ..McpHttpConfig::default()
        };
        let args = daemon_cli_args(4173, &cfg);
        for expected in [
            "--bind=0.0.0.0",
            "--path=/ai/mcp", // normalised: no trailing slash
            "--allow-remote",
            "--allowed-hosts=studio.local,10.0.0.5",
            "--allowed-origins=https://claude.ai",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "missing {expected} in {args:?}"
            );
        }
    }

    #[test]
    fn daemon_cli_args_omit_the_remote_flag_when_it_is_off() {
        // `--allow-remote` 是个 flag，设置关闭时仍传它等于打开用户明确关掉的 LAN 暴露。
        let args = daemon_cli_args(1, &McpHttpConfig::default());
        assert!(!args.iter().any(|a| a == "--allow-remote"), "got {args:?}");
    }

    #[test]
    fn daemon_cli_args_pass_empty_lists_explicitly() {
        // 配合 `--allow-remote`，空 host 名单意味着 daemon *拒绝*所有非 loopback 的
        // `Host`，这与「clap 取了默认值」不是一回事——监管者不能依赖这种巧合。
        let cfg = McpHttpConfig {
            allow_remote: true,
            ..McpHttpConfig::default()
        };
        let args = daemon_cli_args(1, &cfg);
        assert!(args.iter().any(|a| a == "--allow-remote"));
        assert!(args.iter().any(|a| a == "--allowed-hosts="));
        assert!(args.iter().any(|a| a == "--allowed-origins="));
    }

    #[test]
    fn new_handle_snapshot_is_stopped() {
        let h = McpDaemonHandle::new();
        let s = h.snapshot();
        assert!(!s.running, "new handle should be stopped");
        assert_eq!(s.pid, None);
        assert_eq!(s.port, None);
        assert_eq!(s.requests_handled, 0);
        // 不能写 `assert_eq!(.., 0)`：`McpMetrics::new()` 打上
        // `started_at_ms = now_ms()`，`snapshot()` 返回的是时间差，跨过一条语句的
        // 毫秒边界就会读到 1。那是两条相邻语句之间的墙钟差，不是状态变化——精确
        // 断言 0 会在机器忙到被抢占时随机把 CI 弄红。
        assert!(
            s.uptime_ms <= 1,
            "uptime of a handle that was never started should be ~0, got {}",
            s.uptime_ms
        );
        assert!(s.last_error.is_none());
    }

    #[test]
    fn default_matches_new() {
        let h = McpDaemonHandle::default();
        assert!(!h.snapshot().running);
    }

    #[test]
    fn status_strings_match_wire_contract() {
        // renderer 直接按这些字面量字符串分支，保持稳定。
        assert_eq!(DaemonStatus::Stopped.as_str(), "stopped");
        assert_eq!(DaemonStatus::Starting.as_str(), "starting");
        assert_eq!(DaemonStatus::Running.as_str(), "running");
        assert_eq!(DaemonStatus::Restarting.as_str(), "restarting");
        assert_eq!(
            DaemonStatus::Crashed { exit_code: Some(1) }.as_str(),
            "crashed"
        );
        assert_eq!(DaemonStatus::Unhealthy.as_str(), "unhealthy");
    }

    #[test]
    fn crashed_serializes_with_exit_code() {
        // 线格式自检——renderer 依赖 `{ "crashed": { "exit_code": 1 } }`。
        let s = DaemonStatus::Crashed { exit_code: Some(1) };
        let v = serde_json::to_value(s).unwrap();
        let obj = v.as_object().expect("struct variant serializes to object");
        assert!(obj.contains_key("crashed"), "got: {v}");
        let inner = &obj["crashed"];
        assert_eq!(inner["exit_code"], 1);
    }

    #[test]
    fn crashed_with_no_exit_code_uses_null() {
        let s = DaemonStatus::Crashed { exit_code: None };
        let v = serde_json::to_value(s).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj["crashed"]["exit_code"].is_null());
    }
}
