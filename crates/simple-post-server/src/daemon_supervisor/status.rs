//! daemon 生命周期对**外面**长什么样：状态枚举、崩溃事件体，以及把内部记账投影成
//! `McpStatusDto` 的那次快照。与 `super::state` 分开——内部字段随便改，序列化形态
//! 才是契约。

use serde::Serialize;

use super::McpDaemonHandle;

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
    pub(super) fn as_str(self) -> &'static str {
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

impl McpDaemonHandle {
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
            // 这样的行，故这里写死 `0`。先把这个计数器放上线格式里是为了向前兼容——
            // daemon 真的开始发它时，UI 不必改 schema。
            current_connections: 0,
            registered_tools: crate::mcp::registered_tool_names(),
            last_error: state.last_error.clone(),
        }
    }
}
