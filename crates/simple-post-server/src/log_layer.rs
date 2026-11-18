//! 把 `tracing` 事件抄进 core 的日志环形缓冲。
//!
//! 这一层只读事件、只写缓冲区：层内部**绝不**再调用 `tracing` 宏，否则一条事件会递归
//! 生成无限多条事件。
//!
//! 放在 server crate 而不是 core：`tracing-subscriber` 只有 host 这一层需要，core 保持
//! 不依赖它。

use std::fmt::Write as _;

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};

use simple_post_core::logbuf::{self, LogLevel};

/// 挂进 `tracing_subscriber::registry()` 的一层。
#[derive(Debug, Default)]
pub(crate) struct LogBufferLayer;

impl<S: tracing::Subscriber> Layer<S> for LogBufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut fields = FieldVisitor::default();
        event.record(&mut fields);

        // 有 `message` 就用它作正文，其余字段拼成 ` k=v` 尾巴；没有 `message` 时整条记录
        // 只剩结构字段，正文取那串结构字段本身，免得导出一行空记录。
        let message = if fields.message.is_empty() {
            fields.rest.trim_start().to_string()
        } else if fields.rest.is_empty() {
            fields.message
        } else {
            format!("{}{}", fields.message, fields.rest)
        };

        logbuf::record(
            LogLevel::from(*metadata.level()),
            metadata.target(),
            message,
        );
    }
}

/// 事件字段的收集器。
#[derive(Debug, Default)]
struct FieldVisitor {
    message: String,
    /// 结构字段，每段以空格开头，故拼进正文时不必再分隔。
    rest: String,
}

impl FieldVisitor {
    fn push(&mut self, field: &Field, value: &str) {
        let _ = write!(self.rest, " {}={}", field.name(), value);
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.push(field, value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // `tracing::info!("a {}", x)` 的正文走 `record_debug`，值是 `format_args!`，故
        // `{:?}` 打出来就是格式化后的文本（不带引号）。
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = rendered;
        } else {
            self.push(field, &rendered);
        }
    }
}
