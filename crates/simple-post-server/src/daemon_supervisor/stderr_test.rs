//! stderr 逐行分流的两个性质：指标行喂计数器、其余进日志缓冲。
//!
//! **指标行不进日志缓冲**是这里唯一要钉住的东西：那些是机器读的数字，混进用户可见的
//! 日志只会碍眼，而两条路径的分流是逐行发生的，故它必须有一处可测。

use std::sync::Arc;

use crate::mcp_metrics::McpMetrics;

use super::super::log_buffer::LogBuffer;
use super::handle_line;

#[test]
fn a_metric_line_updates_the_counter_and_stays_out_of_the_log() {
    let metrics = McpMetrics::new();
    let logs = LogBuffer::new();
    logs.start_capture();

    handle_line(
        r#"__mcp_metrics__{"requests_handled":7,"errors":0,"uptime_ms":1000}"#,
        &metrics,
        &logs,
    );

    assert_eq!(metrics.snapshot().requests_handled, 7, "指标行要喂计数器");
    assert!(
        logs.snapshot().lines.is_empty(),
        "指标行不该出现在用户可见的日志里：{:?}",
        logs.snapshot().lines
    );
}

#[test]
fn an_ordinary_line_lands_in_the_log_and_leaves_the_counter_alone() {
    let metrics = McpMetrics::new();
    let logs = LogBuffer::new();
    logs.start_capture();

    handle_line("2026-10-20T00:00:00Z INFO daemon ready", &metrics, &logs);

    assert_eq!(
        logs.snapshot().lines,
        vec!["2026-10-20T00:00:00Z INFO daemon ready".to_string()]
    );
    assert_eq!(metrics.snapshot().requests_handled, 0, "普通行不该动计数器");
}

#[test]
fn the_two_kinds_of_lines_do_not_mix() {
    let metrics = McpMetrics::new();
    let logs = LogBuffer::new();
    logs.start_capture();

    handle_line("first", &metrics, &logs);
    handle_line(
        r#"__mcp_metrics__{"requests_handled":1,"errors":0,"uptime_ms":10}"#,
        &metrics,
        &logs,
    );
    handle_line("second", &metrics, &logs);

    assert_eq!(
        logs.snapshot().lines,
        vec!["first".to_string(), "second".to_string()],
        "日志里只有非指标行，且顺序不变"
    );
    assert_eq!(metrics.snapshot().requests_handled, 1);
}

/// 缓冲由多个任务共享（stderr 转发器写、渲染层读），故它必须能安全地跨线程。
#[test]
fn the_buffer_is_shareable_across_threads() {
    let logs = Arc::new(LogBuffer::new());
    logs.start_capture();
    let writer = Arc::clone(&logs);
    let handle = std::thread::spawn(move || {
        for i in 0..100 {
            writer.push(&format!("line {i}"));
        }
    });
    handle.join().expect("writer thread");
    assert_eq!(logs.snapshot().lines.len(), 100);
}
