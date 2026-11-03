//! daemon 的 stderr 捕获：每一行进日志环形缓冲，并转投我们自己的 tracing。

use std::sync::Arc;

use super::log_buffer::LogBuffer;

/// daemon stderr → 日志转发器。读到 EOF（进程退出）为止。
pub(super) async fn forward_stderr(reader: tokio::process::ChildStderr, logs: Arc<LogBuffer>) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => handle_line(&line, &logs),
            Ok(None) => break, // EOF —— 子进程关闭了 stderr
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "stderr read: {e}");
                break;
            }
        }
    }
}

/// 处理一行 stderr：转投我们自己的 tracing 通道（好让开发控制台看到 daemon 的输出），
/// 并留一份在环形缓冲里（好让设置页能 tail 它们）。
fn handle_line(line: &str, logs: &LogBuffer) {
    logs.push(line);
    tracing::debug!(target: "mcp_daemon", "{}", line);
}

#[cfg(test)]
#[path = "stderr_test.rs"]
mod tests;
