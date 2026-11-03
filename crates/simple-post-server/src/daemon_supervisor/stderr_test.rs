//! stderr 逐行分流的性质：每一行都进日志缓冲，并按到达顺序保留。

use std::sync::Arc;

use super::super::log_buffer::LogBuffer;
use super::handle_line;

#[test]
fn an_ordinary_line_lands_in_the_log() {
    let logs = LogBuffer::new();
    logs.start_capture();

    handle_line("2026-10-20T00:00:00Z INFO daemon ready", &logs);

    assert_eq!(
        logs.snapshot().lines,
        vec!["2026-10-20T00:00:00Z INFO daemon ready".to_string()]
    );
}

#[test]
fn the_lines_keep_their_arrival_order() {
    let logs = LogBuffer::new();
    logs.start_capture();

    handle_line("first", &logs);
    handle_line("second", &logs);

    assert_eq!(
        logs.snapshot().lines,
        vec!["first".to_string(), "second".to_string()],
        "日志里按到达顺序保留每一行"
    );
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
