//! 本次运行期间的日志环形缓冲。
//!
//! 进程内保留最近 [`LOG_CAPACITY`] 条日志，供设置面板「导出日志」取走。缓冲区是
//! 全局静态的：`tracing` 层、panic hook 与前端上报三个写入方分属不同模块，没有共同的
//! 所有者可挂。
//!
//! 纪律：写日志的路径不许 panic。锁中毒时记录被丢弃而不是 `unwrap` —— 缓冲区是诊断
//! 设施，它自己变成崩溃源就本末倒置了。

use std::collections::VecDeque;
use std::sync::{Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

/// 缓冲区保留的最大条数。超出即丢最旧的一条。
pub const LOG_CAPACITY: usize = 2000;

/// 单条正文的字节上限。按字符边界截断，否则切在多字节字符中间会产出非法 UTF-8（Rust 的
/// `String` 存不下，只能在 `truncate` 处 panic）。
pub const MESSAGE_LIMIT_BYTES: usize = 8 * 1024;

/// 日志级别。`Warn` 及以上算「错误日志」，导出 `errors` 那一档时只留它们。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// 线上与导出文本里的写法，全大写。
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }

    /// 大小写不敏感，认不出时按 `Info`：级别拼错不该让这条日志写不进去。
    pub fn from_label(label: &str) -> LogLevel {
        match label.trim().to_ascii_lowercase().as_str() {
            "error" => LogLevel::Error,
            "warn" | "warning" => LogLevel::Warn,
            "debug" => LogLevel::Debug,
            "trace" => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }

    /// 是否算「错误日志」。`Warn` 与 `Error` 都算：警告往往正是故障的前兆。
    pub fn is_problem(self) -> bool {
        matches!(self, LogLevel::Error | LogLevel::Warn)
    }
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> LogLevel {
        match level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }
}

/// 一条日志。`at_ms` 是 unix 毫秒，渲染时才格式化成可读时间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub at_ms: u128,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

impl LogRecord {
    /// 导出文本里的一行：`2026-11-18T11:29:09.123Z ERROR target: message`。
    pub fn render_line(&self) -> String {
        format!(
            "{} {} {}: {}",
            rfc3339_millis(self.at_ms),
            self.level.as_str(),
            self.target,
            self.message
        )
    }
}

static BUF: Mutex<VecDeque<LogRecord>> = Mutex::new(VecDeque::new());

/// 追加一条。超过容量时丢最旧的一条；锁中毒时整条丢弃。
pub fn record(level: LogLevel, target: &str, message: impl Into<String>) {
    let entry = LogRecord {
        at_ms: now_ms(),
        level,
        target: target.to_string(),
        message: clamp(message.into()),
    };
    if let Ok(mut buf) = BUF.lock() {
        if buf.len() >= LOG_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(entry);
    }
}

/// 按时间顺序复制一份快照。`errors_only` 只留 [`LogLevel::is_problem`] 的那些。
pub fn snapshot(errors_only: bool) -> Vec<LogRecord> {
    match BUF.lock() {
        Ok(buf) => buf
            .iter()
            .filter(|r| !errors_only || r.level.is_problem())
            .cloned()
            .collect(),
        // 中毒的缓冲区读不出可信内容，返回空而不是 panic —— 调用方拿到空串会显示「暂无日志」。
        Err(_) => Vec::new(),
    }
}

/// 可直接写文件的纯文本。空缓冲区返回空串（末尾没有多余空行）。
pub fn render(errors_only: bool) -> String {
    snapshot(errors_only)
        .iter()
        .map(LogRecord::render_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 文件名用的时间戳：`YYYYMMDD-HHMMSS`（UTC）。
pub fn now_stamp() -> String {
    let secs = (now_ms() / 1000) as u64;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// 安装 panic hook：崩溃先落一条 `ERROR panic` 再交给原有 hook。
///
/// 幂等：重复调用只生效第一次，否则 hook 会层层包裹自己，一次 panic 写多条同样的记录。
pub fn init_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_string());
            let location = info
                .location()
                .map(|l| format!("{file}:{line}", file = l.file(), line = l.line()))
                .unwrap_or_else(|| "unknown location".to_string());
            record(LogLevel::Error, "panic", format!("{payload} at {location}"));
            previous(info);
        }));
    });
}

// 内部

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// 按字符边界截到 [`MESSAGE_LIMIT_BYTES`]。
fn clamp(mut message: String) -> String {
    if message.len() <= MESSAGE_LIMIT_BYTES {
        return message;
    }
    let mut cut = MESSAGE_LIMIT_BYTES;
    while cut > 0 && !message.is_char_boundary(cut) {
        cut -= 1;
    }
    message.truncate(cut);
    message
}

/// unix 毫秒 → `YYYY-MM-DDTHH:MM:SS.mmmZ`。
pub fn rfc3339_millis(ms: u128) -> String {
    let secs = (ms / 1000) as u64;
    let millis = (ms % 1000) as u32;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 `civil_from_days`：unix 日序 → 公历年月日。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}

#[cfg(test)]
#[path = "logbuf_test.rs"]
mod tests;
