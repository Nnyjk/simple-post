//! daemon 的 stderr 捕获：指标行喂进内存计数器，其余进日志环形缓冲并转投我们自己的 tracing。

use std::sync::Arc;

use crate::mcp_metrics::{parse_metric_line, McpMetrics};

use super::log_buffer::LogBuffer;

/// daemon stderr → 指标转发器。读到 EOF（进程退出）为止。
pub(super) async fn forward_stderr_and_parse_metrics(
    reader: tokio::process::ChildStderr,
    metrics: Arc<McpMetrics>,
    logs: Arc<LogBuffer>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => handle_line(&line, &metrics, &logs),
            Ok(None) => break, // EOF —— 子进程关闭了 stderr
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "stderr read: {e}");
                break;
            }
        }
    }
}

/// 处理一行 stderr。
///
/// 指标行喂计数器；其余 presumed 是 daemon 的 tracing / 日志行，转投我们自己的 tracing
/// 通道（好让开发控制台同时看到两路流），并留一份在环形缓冲里（好让设置页能 tail 它们）。
///
/// **指标行不进日志缓冲**：它们是机器读的数字，混进用户可见的日志只会碍眼。
fn handle_line(line: &str, metrics: &McpMetrics, logs: &LogBuffer) {
    if let Some(snap) = parse_metric_line(line) {
        metrics.merge_from_stderr(&snap);
    } else {
        logs.push(line);
        tracing::debug!(target: "mcp_daemon", "{}", line);
    }
}

#[cfg(test)]
#[path = "stderr_test.rs"]
mod tests;
