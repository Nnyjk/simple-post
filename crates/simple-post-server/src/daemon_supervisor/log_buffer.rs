//! daemon 日志的环形缓冲：给设置页一个「最近日志」的 tail。
//!
//! 后端**早就接管了子进程的 stderr**（见 `stderr.rs`），但那份接管只用于两件事：解析
//! `__mcp_metrics__` 指标行、把其余行转投 tracing 通道。没有任何留存，用户看不见 daemon
//! 说过什么。
//!
//! 缓冲是**有界**的：超出容量时丢最旧的行。故长时间运行不会增长。
//!
//! **每次 spawn 重建一个**：重启后的日志不该与上一轮混在一起——用户分辨不出哪一行属于
//! 哪一次运行，那这堆日志就等于没有。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 缓冲的容量上限（行数）。
pub(super) const CAPACITY: usize = 500;

/// 一次 daemon 运行的日志缓冲。
#[derive(Debug)]
pub(super) struct LogBuffer {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    lines: VecDeque<String>,
    /// 因超出容量而丢过行。用户据此知道「上面还有更早的日志，但已经没了」。
    truncated: bool,
    /// 本次 daemon 启动的 Unix 毫秒时间戳。重启后它变，故它就是日志的「分界」。
    started_at_ms: Option<u64>,
    /// 本监管者是否持有这个 daemon 的 stderr 管道。
    ///
    /// 复用一个**外部启动**的 daemon 时没有管道（管道只在 spawn 路径上建立），那种情况下
    /// 缓冲永远是空的。用户看到的应该是「无日志捕获」而不是「没有日志」。
    capturing: bool,
}

/// 一次快照：给渲染层的那份。
#[derive(Debug, Clone, Default)]
pub(super) struct LogTail {
    pub(super) lines: Vec<String>,
    pub(super) started_at_ms: Option<u64>,
    pub(super) truncated: bool,
    pub(super) capturing: bool,
}

impl LogBuffer {
    /// 一个空缓冲。此刻还没有哪个 daemon 的 stderr 管道归我们。
    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                capturing: false,
                ..Inner::default()
            }),
        }
    }

    /// 一次新运行开始：清空旧行、记下分界、开始接管 stderr。
    pub(super) fn start_capture(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.lines.clear();
        inner.truncated = false;
        inner.started_at_ms = Some(now_ms());
        inner.capturing = true;
    }

    /// 本监管者不再持有这个 daemon 的 stderr 管道（复用一个外部启动的 daemon）。
    ///
    /// 那种情况下缓冲永远是空的，用户看到的应该是「无日志捕获」而不是「没有日志」。
    pub(super) fn stop_capture(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.lines.clear();
        inner.truncated = false;
        inner.started_at_ms = None;
        inner.capturing = false;
    }

    /// 记一行。指标行**不进**这里——它们是机器读的数字，混进用户可见的日志只会碍眼。
    pub(super) fn push(&self, line: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.lines.len() >= CAPACITY {
            inner.lines.pop_front();
            inner.truncated = true;
        }
        inner.lines.push_back(line.to_string());
    }

    /// 当前留存的日志。廉价——复制一份字符串列表。
    pub(super) fn snapshot(&self) -> LogTail {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        LogTail {
            lines: inner.lines.iter().cloned().collect(),
            started_at_ms: inner.started_at_ms,
            truncated: inner.truncated,
            capturing: inner.capturing,
        }
    }
}

/// Unix 毫秒时间戳。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "log_buffer_test.rs"]
mod tests;
