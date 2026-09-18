//! Lightweight metrics counters for the MCP daemon.
//!
//! The HTTP daemon emits one `__mcp_metrics__{...}` JSON line to stderr
//! after each handled request. The GUI (and proxy) tail stderr, parse the
//! line, and reflect the live counters in `McpStatusDto` for the
//! renderer.
//!
//! Why stderr and not stdout: the proxy forwards stdout verbatim as
//! JSON-RPC. Anything we write to stdout would corrupt the JSON stream.
//! Stderr is the conventional channel for "diagnostic chatter mixed with
//! structured metrics".
//!
//! Why a tagged prefix: free-form stderr from the daemon (panics,
//! `tracing` logs, future debug prints) shouldn't trigger metrics
//! updates. The `__mcp_metrics__` marker lets the parser skip anything
//! that doesn't match without expensive try-parse on every line.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Live counters. All fields are atomic so reads from the renderer's
/// snapshot path and writes from the daemon's stderr forwarder don't
/// need a mutex.
///
/// `started_at_ms` is set once at construction; the rest are bumped by
/// the daemon (via `inc` / `inc_err`) and the GUI (via the parsed
/// stderr snapshot, when its own counter lags behind the daemon's).
#[derive(Debug, Default)]
pub struct McpMetrics {
    /// Total requests the daemon has handled since boot.
    pub requests_handled: AtomicU64,
    /// Subset of `requests_handled` that errored (HTTP 5xx or tool
    /// error). Independent counter so we can compute a success ratio.
    pub errors: AtomicU64,
    /// Unix epoch milliseconds when the daemon started. Set on
    /// construction; `snapshot()` reads it to compute `uptime_ms`.
    pub started_at_ms: AtomicU64,
}

impl McpMetrics {
    /// Construct with `started_at_ms` set to "now".
    pub fn new() -> Self {
        let s = Self::default();
        s.started_at_ms.store(now_ms(), Ordering::Relaxed);
        s
    }

    /// Increment the success counter.
    pub fn inc(&self) {
        self.requests_handled.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the error counter (separately from success).
    pub fn inc_err(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot of the current counters — what the GUI pushes to the
    /// renderer on every poll. Cheap (three atomic loads).
    pub fn snapshot(&self) -> MetricsSnapshot {
        let started_at_ms = self.started_at_ms.load(Ordering::Relaxed);
        MetricsSnapshot {
            requests_handled: self.requests_handled.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            uptime_ms: now_ms().saturating_sub(started_at_ms),
        }
    }

    /// Apply an incoming stderr snapshot — the GUI uses this to keep
    /// its local counter from drifting backwards relative to the
    /// daemon's truth. We only overwrite if the daemon's count is
    /// ahead; otherwise we keep our own higher value (defensive: the
    /// daemon's view is canonical, but if a brief reconnect delivered
    /// an out-of-order older snapshot we'd rather not rewind the UI).
    pub fn merge_from_stderr(&self, snap: &MetricsSnapshot) {
        let current = self.requests_handled.load(Ordering::Relaxed);
        if snap.requests_handled > current {
            self.requests_handled
                .store(snap.requests_handled, Ordering::Relaxed);
        }
        let current_err = self.errors.load(Ordering::Relaxed);
        if snap.errors > current_err {
            self.errors.store(snap.errors, Ordering::Relaxed);
        }
    }
}

/// Point-in-time view of `McpMetrics`, safe to ship over the IPC wire
/// or stderr. Mirrors the field shape of `McpMetrics` minus the
/// `started_at_ms` raw value (the GUI computes uptime against its own
/// clock anyway, and we want a single number here, not a timestamp).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub requests_handled: u64,
    pub errors: u64,
    pub uptime_ms: u64,
}

/// Wire prefix the daemon prepends to every metrics line. The GUI's
/// parser (`parse_metric_line`) checks this exactly — any line not
/// starting with it is left to `tracing` / console output as-is.
pub const METRIC_PREFIX: &str = "__mcp_metrics__";

/// Daemon-side helper: build the line to write to stderr.
///
/// Format: `__mcp_metrics__{"requests_handled":N,"errors":N,"uptime_ms":N}\n`
///
/// Returns a `String` rather than writing directly so the daemon can
/// route it through `eprintln!` (which flushes per line on a TTY) or
/// through a buffered sink of its choosing.
pub fn emit_metric_line(snap: &MetricsSnapshot) -> String {
    format!(
        "{}{}\n",
        METRIC_PREFIX,
        serde_json::to_string(snap).unwrap_or_default()
    )
}

/// GUI / proxy side: parse one stderr line into a snapshot, or `None`
/// if the line doesn't look like a metrics frame. Trims trailing
/// whitespace so `\r\n` line endings (Windows console) still parse.
pub fn parse_metric_line(line: &str) -> Option<MetricsSnapshot> {
    let trimmed = line.trim();
    let prefix = METRIC_PREFIX;
    if !trimmed.starts_with(prefix) {
        return None;
    }
    let json = &trimmed[prefix.len()..];
    serde_json::from_str::<MetricsSnapshot>(json).ok()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}