//! 监管者的内部记账，与 [`super::status`] 的线格式类型分开。

use std::sync::Arc;
use std::time::Instant;

use super::status::DaemonStatus;
use crate::mcp_metrics::McpMetrics;
use crate::port_registry::PortRecord;

/// 内部：每次 spawn 的记账。放在 tokio mutex 后，公开方法才能是 `&self`
/// （Tauri state 需要共享引用）。
pub(super) struct DaemonState {
    pub(super) pid: Option<u32>,
    pub(super) port: Option<u16>,
    pub(super) port_record: Option<PortRecord>,
    /// 保留句柄，好让 `shutdown` 与 `restart` 能 `kill()` 而不丢它。某些平台上
    /// 子进程已被别处 reap 后再 `wait()` 会报错，所以 `child` 一直留到
    /// `shutdown`，reap 归我们。
    pub(super) child: Option<tokio::process::Child>,
    pub(super) metrics: Arc<McpMetrics>,
    pub(super) status: DaemonStatus,
    pub(super) started_at: Option<Instant>,
    pub(super) last_error: Option<String>,
}

impl DaemonState {
    pub(super) fn empty() -> Self {
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
