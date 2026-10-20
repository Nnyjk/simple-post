//! 日志环形缓冲的三条性质：容量有界、指标行不进缓冲、每次运行各自一份。

use super::{LogBuffer, CAPACITY};

#[test]
fn the_buffer_keeps_at_most_capacity_lines() {
    let buf = LogBuffer::new();
    buf.start_capture();
    for i in 0..(CAPACITY + 50) {
        buf.push(&format!("line {i}"));
    }
    let tail = buf.snapshot();
    assert_eq!(
        tail.lines.len(),
        CAPACITY,
        "缓冲必须有界：长时间运行不能增长"
    );
    assert!(
        tail.truncated,
        "丢过行必须说清楚——用户得知道上面还有更早的日志"
    );
    // 丢的是**最旧**的行：最新的那几行必须还在。
    let last = tail.lines.last().expect("至少一行");
    assert_eq!(last, &format!("line {}", CAPACITY + 49));
    assert_eq!(tail.lines.first().expect("至少一行"), "line 50");
}

#[test]
fn a_fresh_buffer_is_empty_and_not_capturing() {
    let tail = LogBuffer::new().snapshot();
    assert!(tail.lines.is_empty());
    assert!(!tail.capturing, "还没有接管任何 stderr");
    assert!(tail.started_at_ms.is_none());
}

#[test]
fn a_capturing_buffer_says_so() {
    let buf = LogBuffer::new();
    buf.start_capture();
    let tail = buf.snapshot();
    assert!(tail.capturing, "spawn 出来的 daemon 一定有 stderr 管道");
    assert!(tail.started_at_ms.is_some(), "分界靠它");
}

#[test]
fn a_buffer_without_a_pipe_says_so() {
    // 复用一个外部启动的 daemon 时没有管道：那种情况下缓冲永远是空的，
    // 用户看到的应该是「无日志捕获」而不是「没有日志」。
    let buf = LogBuffer::new();
    buf.start_capture();
    buf.push("from the previous run");
    buf.stop_capture();
    let tail = buf.snapshot();
    assert!(!tail.capturing);
    assert!(tail.lines.is_empty());
    assert!(tail.started_at_ms.is_none());
}

#[test]
fn each_run_gets_its_own_buffer() {
    // 重启后的日志不该与上一轮混在一起——用户分辨不出哪行属于哪次运行。
    let buf = LogBuffer::new();
    buf.start_capture();
    buf.push("before restart");
    buf.start_capture();
    buf.push("after restart");
    let tail = buf.snapshot();
    assert_eq!(tail.lines, vec!["after restart".to_string()]);
    assert!(!tail.truncated, "清空过就不算丢过行");
}
