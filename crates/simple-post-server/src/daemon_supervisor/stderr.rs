//! daemon 的 stderr 捕获：指标行喂进内存计数器，其余转投我们自己的 tracing。

use std::sync::Arc;

use crate::mcp_metrics::{parse_metric_line, McpMetrics};

/// daemon stderr → 指标转发器。读到 EOF（进程退出）为止。
pub(super) async fn forward_stderr_and_parse_metrics(
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
