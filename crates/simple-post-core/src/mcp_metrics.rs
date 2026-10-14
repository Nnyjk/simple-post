//! MCP 守护进程的轻量指标计数器。
//!
//! HTTP 守护进程在每个请求处理完后向 stderr 发一行 `__mcp_metrics__{...}` JSON。GUI（与
//! proxy）跟踪 stderr、解析该行，并把活计数器反映到 `McpStatusDto` 供渲染层使用。
//!
//! 为何是 stderr 而非 stdout：proxy 原样转发 stdout 作为 JSON-RPC。我们往 stdout 写的任何
//! 东西都会污染那条 JSON 流。stderr 是「与结构化指标混在一起的诊断杂音」的惯用通道。
//!
//! 为何加标签前缀：守护进程的自由格式 stderr（panic、`tracing` 日志、将来可能有的调试打印）
//! 不该触发指标更新。`__mcp_metrics__` 这个标记让解析器能跳过不匹配的行，而不必对每一行
//! 都做一次昂贵的尝试解析。

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// 活计数器。字段全是原子的，故渲染层快照路径的读与守护进程 stderr 转发侧的写
/// 不需要互斥量。
/// `started_at_ms` 在构造时设置一次；其余由守护进程（经 `inc` / `inc_err`）递增，
/// 也由 GUI 在解析 stderr 快照时递增（当它自己的计数器落后于守护进程时）。
#[derive(Debug, Default)]
pub struct McpMetrics {
    /// 守护进程自启动以来处理过的请求总数。
    pub requests_handled: AtomicU64,
    /// `requests_handled` 中出错的子集（HTTP 5xx 或工具错误）。独立计数以便算成功率。
    pub errors: AtomicU64,
    /// 守护进程启动时的 unix epoch 毫秒。构造时设置；`snapshot()` 读它来算 `uptime_ms`。
    pub started_at_ms: AtomicU64,
}

impl McpMetrics {
    /// 构造，`started_at_ms` 设为「现在」。
    pub fn new() -> Self {
        let s = Self::default();
        s.started_at_ms.store(now_ms(), Ordering::Relaxed);
        s
    }

    /// 递增成功计数。
    pub fn inc(&self) {
        self.requests_handled.fetch_add(1, Ordering::Relaxed);
    }

    /// 递增错误计数（与成功分开记）。
    pub fn inc_err(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// 当前计数器的快照——GUI 每次轮询推给渲染层的就是它。廉价（三次原子读）。
    pub fn snapshot(&self) -> MetricsSnapshot {
        let started_at_ms = self.started_at_ms.load(Ordering::Relaxed);
        MetricsSnapshot {
            requests_handled: self.requests_handled.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            uptime_ms: now_ms().saturating_sub(started_at_ms),
        }
    }

    /// 应用一份来自 stderr 的快照——GUI 用它避免自己的计数器相对守护进程往回退。
    /// 只在守护进程的计数更大时才覆盖，否则保留我们自己的更高值（守护进程的视图是权威，
    /// 但若一次短暂重连送来的是乱序的旧快照，宁可不让 UI 倒退）。
    pub fn merge_from_stderr(&self, snap: &MetricsSnapshot) {
        let current = self.requests_handled.load(Ordering::Relaxed);
        if snap.requests_handled > current {
            self.requests_handled
                .store(snap.requests_handled, Ordering::Relaxed);
        }
        let current_err = self.errors.load(Ordering::Relaxed);
        if snap.errors > current_err {
            self.errors.store(snap.errors, Ordering::Relaxed);
        }
    }
}

/// `McpMetrics` 的时点视图，可安全走 IPC 线或 stderr。镜像 `McpMetrics` 的字段形状，
/// 只是不含 `started_at_ms` 那个原始值（GUI 本就按自己的时钟算 uptime，而这里要的是一个
/// 数字而不是一个时间戳）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub requests_handled: u64,
    pub errors: u64,
    pub uptime_ms: u64,
}

/// 守护进程加在每行指标前的线前缀。GUI 的解析器（`parse_metric_line`）精确比对它——不以它
/// 开头的行原样留给 `tracing` / 控制台输出。
pub const METRIC_PREFIX: &str = "__mcp_metrics__";

/// 守护进程侧辅助函数：构造要写到 stderr 的那一行。
/// 格式：`__mcp_metrics__{"requests_handled":N,"errors":N,"uptime_ms":N}\n`
/// 返回 `String` 而非直接写，好让守护进程可以走 `eprintln!`（在 TTY 上逐行 flush），
/// 或走它自选的带缓冲的 sink。
pub fn emit_metric_line(snap: &MetricsSnapshot) -> String {
    format!(
        "{}{}\n",
        METRIC_PREFIX,
        serde_json::to_string(snap).unwrap_or_default()
    )
}

/// GUI / proxy 侧：把一行 stderr 解析成快照；这一行不像指标帧时返回 `None`。
/// 去掉尾部空白，好让 `\r\n` 行尾（Windows 控制台）也能解析。
pub fn parse_metric_line(line: &str) -> Option<MetricsSnapshot> {
    let trimmed = line.trim();
    let prefix = METRIC_PREFIX;
    if !trimmed.starts_with(prefix) {
        return None;
    }
    let json = &trimmed[prefix.len()..];
    serde_json::from_str::<MetricsSnapshot>(json).ok()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// 测试

#[cfg(test)]
#[path = "mcp_metrics_test.rs"]
mod tests;
