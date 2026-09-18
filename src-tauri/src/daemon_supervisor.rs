//! Supervises the HTTP MCP daemon child process.
//!
//! The Tauri GUI spawns the same binary in `mcp` subcommand mode as a
//! child process, exposes its lifecycle via [`McpDaemonHandle`], and
//! emits a `daemon://crashed` event to the renderer when the child
//! exits unexpectedly.
//!
//! Lifecycle:
//!   * `ensure_started` is called from the GUI's `.setup()` block.
//!     If an existing port file points at a live PID, we reuse it
//!     (cheap restart path for the "user just rebooted their laptop"
//!     case). Otherwise we spawn a fresh child on an OS-assigned port.
//!   * `forward_stderr_and_parse_metrics` tails the child's stderr
//!     line-by-line, feeding metrics lines into the in-memory
//!     `McpMetrics` so `mcp_status` can reflect live numbers.
//!   * `watch_exit` awaits the child and, on non-zero exit, emits
//!     `daemon://crashed` to the Tauri app handle. The renderer shows
//!     a toast and lets the user retry.
//!   * `shutdown` is called on GUI exit. It sends SIGTERM / `child.kill()`
//!     and releases the port file.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::mcp_metrics::{parse_metric_line, McpMetrics};
use crate::port_registry::{self, PortRecord};

/// Lifecycle states the renderer cares about. `#[serde(rename_all =
/// "snake_case")]` keeps the wire shape stable so the React side can
/// switch on the string directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatus {
    /// No process has been spawned yet (initial state).
    Stopped,
    /// Spawn issued but healthz hasn't come back yet.
    Starting,
    /// Healthz green, child PID still alive.
    Running,
    /// User triggered restart, old child killed, new one being spawned.
    Restarting,
    /// Child exited with a non-zero status (or was killed by a signal).
    /// The exit_code is preserved so the renderer can show it.
    Crashed { exit_code: Option<i32> },
    /// Child alive but `/mcp/healthz` is failing — typically a hang or
    /// a deadlock on first request. We don't auto-restart; the user
    /// has to act.
    Unhealthy,
}

impl DaemonStatus {
    fn as_str(self) -> &'static str {
        match self {
            DaemonStatus::Stopped => "stopped",
            DaemonStatus::Starting => "starting",
            DaemonStatus::Running => "running",
            DaemonStatus::Restarting => "restarting",
            DaemonStatus::Crashed { .. } => "crashed",
            DaemonStatus::Unhealthy => "unhealthy",
        }
    }
}

/// Event payload for `daemon://crashed`. Renderer renders a toast with
/// `message` and a "Retry" action that calls `restart_daemon` (Tauri
/// command, see `commands.rs`).
#[derive(Debug, Clone, Serialize)]
pub struct DaemonCrashedPayload {
    pub status: String, // always "crashed" — kept for symmetry w/ DTO
    pub exit_code: Option<i32>,
    pub message: String,
    pub last_error: Option<String>,
}

/// Internal: per-spawn bookkeeping. Lives behind a tokio mutex so the
/// public methods can be `&self` (Tauri state wants shared refs).
struct DaemonState {
    pid: Option<u32>,
    port: Option<u16>,
    port_record: Option<PortRecord>,
    /// Kept so `shutdown` and `restart` can `kill()` without losing the
    /// handle. Some platforms error if `wait()` is called after the
    /// child has already been reaped elsewhere; we keep `child` until
    /// `shutdown` so we own the reap.
    child: Option<tokio::process::Child>,
    metrics: Arc<McpMetrics>,
    status: DaemonStatus,
    started_at: Option<Instant>,
    last_error: Option<String>,
}

impl DaemonState {
    fn empty() -> Self {
        Self {
            pid: None,
            port: None,
            port_record: None,
            child: None,
            metrics: Arc::new(McpMetrics::new()),
            status: DaemonStatus::Stopped,
            started_at: None,
            last_error: None,
        }
    }
}

/// Public handle held in Tauri state. Cloning is cheap (Arc-like) so
/// we can hand copies into spawned tasks.
#[derive(Clone)]
pub struct McpDaemonHandle {
    inner: Arc<Mutex<DaemonState>>,
}

impl Default for McpDaemonHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl McpDaemonHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
        }
    }

    /// Snapshot of the daemon's current state in the shape the renderer
    /// wants. Cheap — just locks and copies. The function signature
    /// returns `crate::commands::McpStatusDto`; if those fields ever
    /// change, this is the one place that needs updating.
    pub fn snapshot(&self) -> crate::commands::McpStatusDto {
        // We use `try_lock` because Tauri's command invocations are
        // synchronous on the main thread; blocking would deadlock when
        // the supervisor's own task is mid-update. If the lock is held
        // we just report "starting" rather than wait — UI truth is
        // allowed to be one frame stale.
        let state = match self.inner.try_lock() {
            Ok(s) => s,
            Err(_) => {
                // Lock contended — tool list unreachable without blocking,
                // so surface none and let the next poll pick up the truth.
                return crate::commands::McpStatusDto {
                    status: DaemonStatus::Starting.as_str().to_string(),
                    running: false,
                    pid: None,
                    port: None,
                    uptime_ms: 0,
                    requests_handled: 0,
                    current_connections: 0,
                    registered_tools: Vec::new(),
                    last_error: Some("supervisor busy".into()),
                };
            }
        };
        let snap = state.metrics.snapshot();
        let status_str = state.status.as_str().to_string();
        // `running` is a derived view of `status` kept for back-compat
        // with any caller that still reads the bool. Only the strict
        // `"running"` state counts — `starting`/`restarting` are
        // transient and the renderer can switch on the lifecycle
        // string for those.
        let running = status_str == "running";
        crate::commands::McpStatusDto {
            status: status_str,
            running,
            pid: state.pid,
            port: state.port,
            uptime_ms: snap.uptime_ms,
            requests_handled: snap.requests_handled,
            // `current_connections` is reserved for a future stderr
            // metrics line; the daemon does not emit one yet, so we
            // hard-code `0`. Surfacing a counter here now means the
            // wire shape is forward-compatible — when the daemon
            // starts emitting it the UI doesn't need a schema bump.
            current_connections: 0,
            registered_tools: crate::mcp::registered_tool_names(),
            last_error: state.last_error.clone(),
        }
    }

    /// Idempotent boot:
    ///   1. If `port_registry::read()` returns a record whose PID is
    ///      still alive AND `/mcp/healthz` answers → reuse. This is
    ///      the "user rebooted their laptop and the daemon didn't
    ///      clean up its port file" case.
    ///   2. Otherwise release any stale file, claim a fresh port,
    ///      spawn the process, write the port record with the new
    ///      PID, and wait up to 5s for `/mcp/healthz` to come green.
    ///   3. Spawn the stderr-forwarder and exit-watcher as background
    ///      tasks; they outlive this function and update state until
    ///      the child dies.
    pub async fn ensure_started(
        &self,
        app_handle: &AppHandle,
    ) -> Result<u16, crate::error::AppError> {
        // ----- Reuse path ------------------------------------------------
        if let Some(rec) = port_registry::read() {
            if port_registry::is_pid_alive(rec.pid) && port_registry::healthz(rec.port).await {
                let mut g = self.inner.lock().await;
                g.pid = Some(rec.pid);
                g.port = Some(rec.port);
                g.port_record = Some(rec.clone());
                g.status = DaemonStatus::Running;
                g.started_at = Some(Instant::now());
                return Ok(rec.port);
            }
        }

        // ----- Fresh spawn path ------------------------------------------
        let mut g = self.inner.lock().await;
        // Re-check under the lock: another task may have just spawned
        // while we were awaiting the lock above.
        if let Some(rec) = port_registry::read() {
            if port_registry::is_pid_alive(rec.pid) && port_registry::healthz(rec.port).await {
                g.pid = Some(rec.pid);
                g.port = Some(rec.port);
                g.port_record = Some(rec.clone());
                g.status = DaemonStatus::Running;
                g.started_at = Some(Instant::now());
                return Ok(rec.port);
            }
        }
        // Status now: Starting. Anything that fails below flips us to
        // Crashed / Unhealthy so the UI can reflect it.
        g.status = DaemonStatus::Starting;
        g.last_error = None;
        drop(g); // release before we await on spawn (tokio::process::Command needs &mut self)

        // Clear the stale file before we pick a new port so the read
        // path inside port_registry::write() doesn't bounce off it.
        let _ = port_registry::release();

        // ----- Allocate a port ------------------------------------------
        // Bind port 0 → OS picks. The actual port comes out of
        // local_addr(); we then drop the listener before spawning so
        // the daemon can grab it (race-free on Linux because we hold
        // the lockfile and only one GUI claims it, but a real TOCTOU
        // would require two GUIs starting at the same instant).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(crate::error::AppError::Io)?;
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .map_err(crate::error::AppError::Io)?;
        drop(listener);

        // ----- Spawn the child -------------------------------------------
        let exe = std::env::current_exe().map_err(crate::error::AppError::Io)?;
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("mcp")
            .arg("--http")
            .arg(format!("--port={port}"))
            .arg("--bind=127.0.0.1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true); // GUI drop ⇒ child gets SIGKILL

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let mut g = self.inner.lock().await;
                g.status = DaemonStatus::Crashed { exit_code: None };
                g.last_error = Some(format!("spawn failed: {e}"));
                return Err(crate::error::AppError::Io(e));
            }
        };
        let pid = child.id().unwrap_or(0);
        let stderr = child.stderr.take();

        let data_dir = dirs::data_dir()
            .map(|p| p.join("simple-post").display().to_string())
            .unwrap_or_default();

        let rec = PortRecord::new(pid, port, "127.0.0.1", data_dir);
        if let Err(e) = port_registry::write(&rec) {
            // Port file write failed — we own the child; reap it before
            // returning so we don't leak a daemon with no port file.
            let _ = child.kill().await;
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Crashed { exit_code: None };
            g.last_error = Some(format!("port registry: {e:?}"));
            return Err(e);
        }

        // ----- Update public state ---------------------------------------
        {
            let mut g = self.inner.lock().await;
            g.pid = Some(pid);
            g.port = Some(port);
            g.port_record = Some(rec);
            g.status = DaemonStatus::Starting;
            g.started_at = Some(Instant::now());
            // NOTE: keep `g.metrics` from before — it may have been
            // updated by an earlier session and is reused if we ever
            // recycle this handle across restarts in a long-lived run.
        }

        // ----- Wait for healthz ------------------------------------------
        let mut healthy = false;
        for _ in 0..50 {
            // 50 × 100ms = 5s
            if port_registry::healthz(port).await {
                healthy = true;
                break;
            }
            // If the child already exited, bail out fast — no point
            // burning the whole 5s on a process that died immediately.
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        if !healthy {
            // Don't kill yet — the exit-watcher will do that and emit
            // the right Tauri event. We just flip the visible state.
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Unhealthy;
            g.last_error = Some("healthz never came up within 5s".into());
            // Hand the child over to the watcher before returning.
            g.child = Some(child);
            let app = app_handle.clone();
            let inner = self.inner.clone();
            tokio::spawn(async move {
                watch_exit(inner, app, pid).await;
            });
            return Err(crate::error::AppError::Validation(
                "mcp daemon healthz timeout".into(),
            ));
        }

        // ----- Hand off to background tasks ------------------------------
        let metrics = {
            let g = self.inner.lock().await;
            Arc::clone(&g.metrics)
        };
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr_and_parse_metrics(stderr, metrics));
        }
        let app = app_handle.clone();
        let inner = self.inner.clone();
        {
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Running;
            g.child = Some(child);
        }
        tokio::spawn(async move {
            watch_exit(inner, app, pid).await;
        });

        Ok(port)
    }

    /// Restart: kill the running child (if any), wait briefly for the
    /// OS to free the port, then `ensure_started` again. Returns the
    /// post-restart `McpStatusDto` so the Tauri command can echo the
    /// new `pid` / `port` back to the renderer without an extra poll.
    pub async fn restart(
        &self,
        app_handle: &AppHandle,
    ) -> Result<crate::commands::McpStatusDto, crate::error::AppError> {
        {
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Restarting;
            if let Some(mut child) = g.child.take() {
                let _ = child.start_kill();
            }
        }
        // Give the OS a moment to release the port. 200ms is plenty on
        // Windows; macOS/Linux can sometimes need a touch longer under
        // TIME_WAIT but the daemon process exiting already moves us
        // out of the bind queue.
        sleep(Duration::from_millis(200)).await;
        let _ = port_registry::release();
        // We deliberately ignore the inner `u16` return from
        // `ensure_started` — the caller wants the full snapshot, and
        // the snapshot already carries the new `port` field.
        let _ = self.ensure_started(app_handle).await?;
        Ok(self.snapshot())
    }

    /// Best-effort shutdown. Called from the GUI's exit hook (and from
    /// tests). We do NOT block on `child.wait()` — the watcher task
    /// will reap the child asynchronously and the GUI is on its way
    /// out anyway.
    pub async fn shutdown(&self) -> Result<(), crate::error::AppError> {
        let mut g = self.inner.lock().await;
        if let Some(mut child) = g.child.take() {
            let _ = child.start_kill();
        }
        g.status = DaemonStatus::Stopped;
        g.pid = None;
        g.port = None;
        g.port_record = None;
        drop(g);
        port_registry::release()
    }
}

/// Daemon stderr → metrics forwarder. Runs until EOF (process exit).
async fn forward_stderr_and_parse_metrics(
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
                    // Anything that isn't a metrics line is presumably
                    // a tracing/log line from the daemon — surface it
                    // on our own tracing channel so dev consoles see
                    // both streams.
                    tracing::debug!(target: "mcp_daemon", "{}", line);
                }
            }
            Ok(None) => break, // EOF — child closed stderr
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "stderr read: {e}");
                break;
            }
        }
    }
}

/// Awaits the child's exit; updates state and emits the Tauri event.
/// Holds its own Arc clone of the supervisor's mutex so this task
/// survives any intermediate restarts cleanly.
async fn watch_exit(
    inner: Arc<Mutex<DaemonState>>,
    app_handle: AppHandle,
    pid: u32,
) {
    // F-D1 fix: take the Child OUT of state inside the lock, then drop
    // the guard before awaiting `wait()`. The previous version held
    // `inner.lock().await` across `child.wait().await`, which permanently
    // deadlocked `mcp_daemon_restart` — restart()'s `self.inner.lock().await`
    // would queue behind this watcher forever while a healthy daemon sat
    // there. With the take-out pattern the guard drops at the end of the
    // block, leaving wait() lock-free so other callers (restart, shutdown,
    // snapshot) can proceed.
    let mut child = {
        let mut g = inner.lock().await;
        match g.child.take() {
            Some(c) => c,
            None => {
                // Supervisor tore down the child out from under us
                // (e.g. shutdown() called); nothing to do.
                return;
            }
        }
    };
    let exit_status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "mcp_daemon", "wait() failed: {e}");
            return;
        }
    };

    let exit_code = exit_status.code();
    let message = if let Some(code) = exit_code {
        format!("daemon exited with code {code}")
    } else {
        "daemon terminated by signal".to_string()
    };

    let payload = DaemonCrashedPayload {
        status: "crashed".into(),
        exit_code,
        message: message.clone(),
        last_error: None,
    };

    {
        let mut g = inner.lock().await;
        // Only flip the status if this watcher is still observing the
        // child whose PID matches the current supervisor PID. A
        // restart would have replaced both.
        if g.pid == Some(pid) {
            g.status = DaemonStatus::Crashed { exit_code };
            g.last_error = Some(message.clone());
            g.pid = None;
            g.port = None;
            g.port_record = None;
        }
    }
    // Best-effort event emission — UI will catch up on next mcp_status
    // poll even if the event bus is closed for some reason.
    if let Err(e) = app_handle.emit("daemon://crashed", &payload) {
        eprintln!("[daemon] emit daemon://crashed failed: {e}");
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_handle_snapshot_is_stopped() {
        let h = McpDaemonHandle::new();
        let s = h.snapshot();
        assert!(!s.running, "new handle should be stopped");
        assert_eq!(s.pid, None);
        assert_eq!(s.port, None);
        assert_eq!(s.requests_handled, 0);
        assert_eq!(s.uptime_ms, 0);
        assert!(s.last_error.is_none());
    }

    #[test]
    fn default_matches_new() {
        let h = McpDaemonHandle::default();
        assert!(!h.snapshot().running);
    }

    #[test]
    fn status_strings_match_wire_contract() {
        // Renderer switches on these literal strings; keep them stable.
        assert_eq!(DaemonStatus::Stopped.as_str(), "stopped");
        assert_eq!(DaemonStatus::Starting.as_str(), "starting");
        assert_eq!(DaemonStatus::Running.as_str(), "running");
        assert_eq!(DaemonStatus::Restarting.as_str(), "restarting");
        assert_eq!(
            DaemonStatus::Crashed { exit_code: Some(1) }.as_str(),
            "crashed"
        );
        assert_eq!(DaemonStatus::Unhealthy.as_str(), "unhealthy");
    }

    #[test]
    fn crashed_serializes_with_exit_code() {
        // Wire shape sanity check — the renderer cares about
        // `{ "crashed": { "exit_code": 1 } }`.
        let s = DaemonStatus::Crashed { exit_code: Some(1) };
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().expect("struct variant serializes to object");
        assert!(obj.contains_key("crashed"), "got: {v}");
        let inner = &obj["crashed"];
        assert_eq!(inner["exit_code"], 1);
    }

    #[test]
    fn crashed_with_no_exit_code_uses_null() {
        let s = DaemonStatus::Crashed { exit_code: None };
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj["crashed"]["exit_code"].is_null());
    }
}