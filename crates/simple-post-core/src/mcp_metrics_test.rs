use super::*;

#[test]
fn new_sets_started_at_to_now() {
    let before = now_ms();
    let m = McpMetrics::new();
    let after = now_ms();
    let s = m.started_at_ms.load(Ordering::Relaxed);
    assert!(s >= before && s <= after, "{s} not in [{before}, {after}]");
}

#[test]
fn inc_and_inc_err_update_independent_counters() {
    let m = McpMetrics::new();
    m.inc();
    m.inc();
    m.inc_err();
    let s = m.snapshot();
    assert_eq!(s.requests_handled, 2);
    assert_eq!(s.errors, 1);
}

#[test]
fn snapshot_uptime_is_non_negative() {
    let m = McpMetrics::new();
    // Tiny sleep to let the clock tick — guard against a clock that
    // hasn't advanced since `new`.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let s = m.snapshot();
    assert!(s.uptime_ms >= 1, "uptime_ms = {}", s.uptime_ms);
}

#[test]
fn merge_from_stderr_advances_only_when_higher() {
    let m = McpMetrics::new();
    m.inc(); // local = 1
    let snap_fresh = MetricsSnapshot {
        requests_handled: 5,
        errors: 0,
        uptime_ms: 500,
    };
    m.merge_from_stderr(&snap_fresh);
    assert_eq!(m.requests_handled.load(Ordering::Relaxed), 5);

    // Stale snapshot must not rewind.
    let snap_stale = MetricsSnapshot {
        requests_handled: 2,
        errors: 0,
        uptime_ms: 100,
    };
    m.merge_from_stderr(&snap_stale);
    assert_eq!(m.requests_handled.load(Ordering::Relaxed), 5);
}

#[test]
fn emit_then_parse_round_trip() {
    let snap = MetricsSnapshot {
        requests_handled: 42,
        errors: 3,
        uptime_ms: 1234,
    };
    let line = emit_metric_line(&snap);
    // Prefix check: line must start with our marker so consumers can
    // filter cheaply.
    assert!(line.starts_with(METRIC_PREFIX));
    assert!(line.ends_with('\n'));

    let parsed = parse_metric_line(&line).expect("parse own emit");
    assert_eq!(parsed.requests_handled, 42);
    assert_eq!(parsed.errors, 3);
    assert_eq!(parsed.uptime_ms, 1234);
}

#[test]
fn parse_returns_none_for_unrelated_line() {
    assert!(parse_metric_line("[INFO] starting up").is_none());
    assert!(parse_metric_line("").is_none());
    assert!(parse_metric_line("__other_prefix__{}").is_none());
}

#[test]
fn parse_handles_crlf_line_endings() {
    // Windows consoles sometimes inject \r before \n; we must
    // still recognize the frame.
    let snap = MetricsSnapshot {
        requests_handled: 7,
        errors: 0,
        uptime_ms: 100,
    };
    let mut line = emit_metric_line(&snap);
    line.pop(); // drop the trailing \n
    line.push_str("\r\n");
    let parsed = parse_metric_line(&line).expect("parse crlf");
    assert_eq!(parsed.requests_handled, 7);
}

#[test]
fn parse_returns_none_on_garbage_json() {
    let line = format!("{}{}", METRIC_PREFIX, "{not really json}\n");
    assert!(parse_metric_line(&line).is_none());
}
