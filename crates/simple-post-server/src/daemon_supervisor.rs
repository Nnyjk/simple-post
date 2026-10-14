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
//!   * `stop` is the manual OFF switch (Tauri `mcp_daemon_stop` /
//!     `POST /api/mcp/stop`); `shutdown` is the app-exit hook. Both
//!     go through the same `teardown`, which also terminates a daemon
//!     this supervisor merely *adopted* — see [`teardown_targets`].
//!   * `start` is the manual ON switch and is idempotent: a healthy
//!     tracked daemon is returned untouched rather than respawned.
//!
//! `auto_start` (a persisted preference) and `start`/`stop` (runtime
//! verbs) compose rather than overlap: the preference decides whether
//! the daemon comes up on launch, the manual switch decides whether it
//! is up *now*. Disabling `auto_start` never kills a running daemon.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
// Only the GUI host emits `daemon://crashed`. Behind this feature the
// crate has no Tauri dependency at all, which is what keeps the
// container image free of GTK/WebKit and the `--lib` test binary free
// of the Windows GUI runtime.
#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::domain::McpHttpConfig;
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
/// command, see `commands/mcp_status.rs`).
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
/// 构造守护进程的真实 CLI 参数，桌面端与 Web 端共享同一份契约。
///
/// Everything but the port comes from the persisted `mcp.http_config`
/// row — the daemon used to be spawned with `--bind=127.0.0.1` and a
/// hardcoded `/mcp`, which meant a user who opened the daemon to the LAN
/// had to hand-edit the child's command line. Reading the row here means
/// the settings panel and the child process cannot disagree.
///
/// `port` is the OS-assigned port the supervisor just claimed; it is
/// passed explicitly rather than letting the child re-pick, because the
/// supervisor writes that same port into `mcp.port` and health-probes it.
/// The child's own `mcp.http_config.port` is therefore only a default for
/// a hand-run `simple-post mcp`.
///
/// The empty allowlists are passed as `--allowed-hosts=` /
/// `--allowed-origins=`, which clap parses as an empty list. That is
/// meaningful, not filler: an empty host list with `--allow-remote` is
/// the daemon denying every non-loopback `Host`, and omitting the flag
/// would rely on the same default arriving the same way.
fn daemon_cli_args(port: u16, config: &McpHttpConfig) -> Vec<String> {
    let mut args = vec![
        "mcp".to_string(),
        "--transport=http".to_string(),
        format!("--port={port}"),
        format!("--bind={}", config.host),
        format!("--path={}", config.normalized_path()),
    ];
    if config.allow_remote {
        args.push("--allow-remote".to_string());
    }
    args.push(format!(
        "--allowed-hosts={}",
        config.allowed_hosts.join(",")
    ));
    args.push(format!(
        "--allowed-origins={}",
        config.allowed_origins.join(",")
    ));
    args
}

/// How long teardown waits for a killed daemon to actually leave the
/// process table before the port file is released and a replacement is
/// spawned. This replaces a flat 200ms sleep, which was a guess: on
/// Windows the child could still be alive at that point, and a socket
/// that has not finished closing can keep the port out of the bind
/// queue on macOS/Linux. Bounded on purpose — a wedged process must not
/// hang the UI's start/stop button forever.
const TEARDOWN_TIMEOUT: Duration = Duration::from_millis(2_000);
/// Poll cadence inside [`TEARDOWN_TIMEOUT`].
const TEARDOWN_POLL: Duration = Duration::from_millis(50);

/// Host-side data directory used by the Tauri shape, which has no
/// data-dir parameter to thread through. Matches what the daemon child
/// derives for itself; the `serve-web` shape passes its own explicitly.
#[cfg(feature = "gui")]
fn host_data_dir() -> String {
    dirs::data_dir()
        .map(|p| p.join("simple-post").display().to_string())
        .unwrap_or_default()
}

/// What a freshly spawned child needs from its host once the shared
/// spawn path returns.
struct PendingWatch {
    inner: Arc<Mutex<DaemonState>>,
    pid: u32,
}

/// Result of [`McpDaemonHandle::ensure_started_with`] — the one
/// host-agnostic spawn path.
///
/// # Why the Tauri binding is a *method on this type* and not a
/// parameter of the spawn path
///
/// This started as an `ExitNotifier` enum that *owned* an `AppHandle`,
/// which the spawn path took by reference and cloned into the
/// background exit-watcher task. That compiles, passes review, and
/// works — until someone runs `cargo test -p simple-post --lib`, whose
/// test binary then dies at **load** time with
/// `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` and zero tests run. The
/// unresolvable entrypoint is `comctl32.dll!TaskDialogIndirect`, which
/// only exists in comctl32 **v6** (a side-by-side assembly resolved via
/// an activation context, not the v5 copy in `System32`). It reaches the
/// test binary because an `AppHandle` travelling through shared,
/// `tokio::spawn`ed code keeps the whole Tauri Windows runtime
/// reachable from this module, and with it `tao`/`muda` — five extra
/// DLLs and +2.4 MB of GUI/COM/shell code that the `--lib` test build
/// has no business linking.
///
/// The coupling is therefore split the other way round: the spawn path
/// stays 100% Tauri-free and returns *what it spawned*; only this thin
/// `finish` pair knows about `AppHandle`, and it is reachable solely
/// from the three Tauri bindings (`ensure_started`, `start`,
/// `restart`), exactly the shape that worked before the refactor.
/// **Do not move an `AppHandle` back into a type or a parameter that
/// `ensure_started_with` can see.**
struct Started {
    port: u16,
    /// `Some` only when a fresh child was spawned and still needs an
    /// exit-watcher; `None` on the reuse path (no new process, nothing
    /// to watch).
    watch: Option<PendingWatch>,
}

impl Started {
    /// Tauri shape: arm the watcher that emits `daemon://crashed`.
    #[cfg(feature = "gui")]
    fn tauri_finish(self, app_handle: &AppHandle) -> u16 {
        let port = self.port;
        if let Some(w) = self.watch {
            let app = app_handle.clone();
            tokio::spawn(async move {
                watch_exit(w.inner, app, w.pid).await;
            });
        }
        port
    }

    /// `serve-web` shape: same watcher, minus the Tauri event emission.
    /// The web UI learns about a dead daemon from the `/api/mcp/status`
    /// poll instead.
    fn poll_only_finish(self) -> u16 {
        let port = self.port;
        if let Some(w) = self.watch {
            tokio::spawn(async move {
                watch_exit_no_app(w.inner, w.pid).await;
            });
        }
        port
    }
}

/// One PID a lifecycle verb must terminate, plus how it can be reaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeardownTarget {
    pub pid: u32,
    /// `true` when the supervisor holds a `Child` handle for this PID —
    /// we spawned it, so we own the reap and can use `try_wait()`.
    /// `false` for an *adopted* daemon named only by the port record:
    /// there is no handle, so the PID has to be killed out-of-band via
    /// [`port_registry::terminate_pid`].
    pub via_handle: bool,
}

/// Pure: which PIDs a teardown must terminate.
///
/// Two channels, one PID each:
///   * `owned_pid` — the child we spawned and still hold a handle for.
///   * `adopted_pid` — a live daemon named by the port record that we
///     never spawned. This is the case that made "restart" a no-op:
///     `ensure_started`'s reuse path fills `state.pid` but never sets
///     `state.child`, so the old `if let Some(child) = g.child.take()`
///     was `None` and `restart` immediately re-adopted the very process
///     it was supposed to replace, while reporting success.
///
/// A PID appears at most once (a daemon we both spawned and adopted is
/// the same process, and the handle is the cheaper way to reap it).
/// `0` is filtered out here rather than at each call site — `child.id()`
/// legitimately returns `None` right after spawn, and a `0` target
/// would otherwise reach `taskkill /F /PID 0`.
fn teardown_targets(owned_pid: Option<u32>, adopted_pid: Option<u32>) -> Vec<TeardownTarget> {
    let mut out = Vec::new();
    if let Some(pid) = owned_pid.filter(|p| *p != 0) {
        out.push(TeardownTarget {
            pid,
            via_handle: true,
        });
    }
    if let Some(pid) = adopted_pid.filter(|p| *p != 0 && Some(*p) != owned_pid) {
        out.push(TeardownTarget {
            pid,
            via_handle: false,
        });
    }
    out
}

/// Pure: is a "start" request already satisfied?
///
/// This is the idempotency rule for the manual ON switch: if the
/// supervisor is tracking a daemon that still answers healthz, a start
/// returns that daemon's identity rather than killing and respawning it.
/// `tracked_port.is_some()` is required alongside the status because
/// `Running` with no tracked port is the "adopted, then lost the port
/// record" state — liveness cannot be confirmed there, and a fresh
/// spawn is the only honest answer.
// Pure decision function with no Tauri in it, so it stays compiled
// unconditionally — `daemon_supervisor_test.rs` exercises it directly.
// Only its production caller (`is_serving`) is GUI-gated, hence the
// `allow(dead_code)` on a feature that has no GUI.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
fn start_is_noop(status: DaemonStatus, tracked_port: Option<u16>, healthy: bool) -> bool {
    status == DaemonStatus::Running && tracked_port.is_some() && healthy
}

/// Pure: does this exit-watcher still own the supervisor state?
///
/// Both `watch_exit` variants flip shared state to `Crashed` only while
/// the daemon they are watching is still the one the state points at.
/// `teardown` clears `pid` *before* killing, so a watcher woken by the
/// teardown's own kill sees `None` and stays quiet instead of
/// overwriting the `Restarting`/`Stopped` status the verb just set — or,
/// worse, the fresh state a subsequent start installs on the same
/// handle. Keep this as a function rather than an inline comparison so
/// the guard is unit-tested; it is the only thing standing between a
/// deliberate teardown and a spurious "crashed" toast.
fn watcher_owns_state(state_pid: Option<u32>, watched_pid: u32) -> bool {
    state_pid == Some(watched_pid)
}

/// Bounded wait for an owned child to be reaped.
///
/// Returns `true` when `try_wait()` observed the exit inside `budget`.
/// Cheap on both host shapes — no syscall beyond the wait itself — which
/// is why the handle-based path polls far tighter than the PID one.
async fn wait_for_child_exit(child: &mut tokio::process::Child, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(target: "mcp_daemon", "try_wait during teardown failed: {e}");
                return false;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(TEARDOWN_POLL).await;
    }
}

/// Bounded wait for a PID we hold no handle for to leave the process
/// table. Coarser than [`wait_for_child_exit`] because
/// [`port_registry::is_pid_alive`] costs a process spawn on Windows
/// (`tasklist`), so polling it at 50ms would dominate the teardown.
async fn wait_for_pid_exit(pid: u32, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if !port_registry::is_pid_alive(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(TEARDOWN_POLL).await;
    }
}

/// Public handle held in Tauri state. Cloning is cheap (Arc-like) so
/// we can hand copies into spawned tasks.
#[derive(Clone)]
pub struct McpDaemonHandle {
    inner: Arc<Mutex<DaemonState>>,
    /// The pool used to read `mcp.http_config` at spawn time, so the
    /// child's CLI matches what the settings panel shows.
    ///
    /// `None` on the bare `new()` used by tests, which then falls back
    /// to [`McpHttpConfig::default`]. Both real host shapes
    /// (`lib.rs::run` and `web_api::serve`) construct through
    /// [`Self::with_pool`] — the whole point of reading the row at
    /// *spawn* time rather than at construction is that the user can
    /// save new settings and then hit restart.
    ///
    /// `r2d2::Pool` is `Arc`-backed, so this clone is cheap.
    pool: Option<crate::db::DbPool>,
}

impl Default for McpDaemonHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl McpDaemonHandle {
    /// A handle with no DB access — spawns fall back to the default
    /// listener configuration. Kept for tests and for any embedder that
    /// drives the supervisor without a database.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
            pool: None,
        }
    }

    /// The constructor both real host shapes use.
    pub fn with_pool(pool: crate::db::DbPool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonState::empty())),
            pool: Some(pool),
        }
    }

    /// Snapshot of the daemon's current state in the shape the renderer
    /// wants. Cheap — just locks and copies. The function signature
    /// returns `crate::dto::McpStatusDto`; if those fields ever
    /// change, this is the one place that needs updating.
    pub fn snapshot(&self) -> crate::dto::McpStatusDto {
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
                return crate::dto::McpStatusDto {
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
        crate::dto::McpStatusDto {
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

    /// Idempotent boot (Tauri shape). Thin binding of
    /// [`Self::ensure_started_with`]: this is the signature the GUI's
    /// `.setup()` block and `mcp_auto_start_set` call.
    #[cfg(feature = "gui")]
    pub async fn ensure_started(
        &self,
        app_handle: &AppHandle,
    ) -> Result<u16, crate::error::AppError> {
        Ok(self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle))
    }

    /// [`Self::ensure_started`] for the `serve-web` shape, which has no
    /// `AppHandle` and knows its own data dir.
    pub async fn ensure_started_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<u16, crate::error::AppError> {
        Ok(self
            .ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish())
    }

    /// The single spawn path shared by every host shape.
    ///
    /// Deliberately free of `AppHandle` (and of anything else Tauri) so
    /// the `--lib` test binary does not link the GUI runtime — see the
    /// long note on [`Started`] before changing its signature.
    ///
    ///   1. If `port_registry::read()` returns a record whose PID is
    ///      still alive AND `/mcp/healthz` answers → reuse. This is
    ///      the "user rebooted their laptop and the daemon didn't
    ///      clean up its port file" case. The reused daemon is
    ///      *adopted*: `state.pid` is recorded but `state.child` stays
    ///      `None`, because the supervisor never spawned it and so has
    ///      no reap handle — see [`teardown_targets`] for what that
    ///      implies for stop/restart. No watcher is handed back either:
    ///      there is no child handle for it to observe.
    ///   2. Otherwise release any stale file, claim a fresh port,
    ///      spawn the process, write the port record with the new
    ///      PID, and wait up to 5s for `/mcp/healthz` to come green.
    ///   3. Spawn the stderr-forwarder here, and return the
    ///      `(state, pid)` pair the *caller* turns into its own
    ///      exit-watcher. That task outlives this function and updates
    ///      state until the child dies.
    async fn ensure_started_with(&self, data_dir: &str) -> Result<Started, crate::error::AppError> {
        // ----- Reuse path ------------------------------------------------
        if let Some(rec) = port_registry::read() {
            if port_registry::is_pid_alive(rec.pid) && port_registry::healthz(rec.port).await {
                let mut g = self.inner.lock().await;
                g.pid = Some(rec.pid);
                g.port = Some(rec.port);
                g.port_record = Some(rec.clone());
                g.status = DaemonStatus::Running;
                g.started_at = Some(Instant::now());
                return Ok(Started {
                    port: rec.port,
                    watch: None,
                });
            }
        }

        // ----- Fresh spawn path ------------------------------------------
        // Resolved once, up front: it feeds both the child's command line
        // and the port record, and the two must agree — a record that
        // claimed `127.0.0.1` for a daemon actually bound to `0.0.0.0`
        // would make `requires_restart` answer "always" on every save.
        let http_config = self.http_config();
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
                return Ok(Started {
                    port: rec.port,
                    watch: None,
                });
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
        cmd.args(daemon_cli_args(port, &http_config))
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

        let rec = PortRecord::new(pid, port, &http_config.host, data_dir.to_string());
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
            // Keep the child in `state` rather than handing it to a
            // watcher: the spawn path cannot arm one without knowing the
            // host (that is the whole point of [`Started`]), and a
            // watcher that *took* the handle would strip `state.child`
            // and leave `stop`/`restart` unable to reap this process.
            // Leaving it here keeps the `owned_pid` channel of
            // [`teardown_targets`] working for an unhealthy start too.
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Unhealthy;
            g.last_error = Some("healthz never came up within 5s".into());
            g.child = Some(child);
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
        {
            let mut g = self.inner.lock().await;
            g.status = DaemonStatus::Running;
            g.child = Some(child);
        }

        // The watcher is the caller's to arm: it is the one piece of
        // host behaviour this path refuses to know about.
        Ok(Started {
            port,
            watch: Some(PendingWatch {
                inner: self.inner.clone(),
                pid,
            }),
        })
    }

    /// Manual ON switch (Tauri shape). Idempotent by construction: a
    /// healthy tracked daemon is returned untouched — no kill, no
    /// respawn, no port churn — and only a genuinely stopped / crashed /
    /// unhealthy daemon pays the spawn path.
    #[cfg(feature = "gui")]
    pub async fn start(
        &self,
        app_handle: &AppHandle,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        if self.is_serving().await {
            return Ok(self.snapshot());
        }
        // We deliberately ignore the returned port: the caller wants the
        // whole snapshot, which already carries the new `port`.
        let _ = self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle);
        Ok(self.snapshot())
    }

    /// [`Self::start`] for the `serve-web` shape.
    pub async fn start_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish();
        Ok(self.snapshot())
    }

    /// Manual OFF switch. Terminates the daemon we spawned *and* one we
    /// merely adopted, releases the port file, and leaves the state at
    /// `Stopped` with `pid`/`port` cleared — so the next `start` runs
    /// the fresh-spawn path cleanly. Safe to call when nothing is
    /// running: every teardown step is a no-op in that case.
    pub async fn stop(&self) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Stopped).await?;
        Ok(self.snapshot())
    }

    /// Restart: tear the daemon down completely (see [`Self::teardown`]),
    /// then spawn a fresh one. Returns the post-restart
    /// `McpStatusDto` so the caller can echo the new `pid` / `port` back
    /// without an extra poll.
    #[cfg(feature = "gui")]
    pub async fn restart(
        &self,
        app_handle: &AppHandle,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Restarting).await?;
        let _ = self
            .ensure_started_with(&host_data_dir())
            .await?
            .tauri_finish(app_handle);
        Ok(self.snapshot())
    }

    /// [`Self::restart`] for the `serve-web` shape, which has no
    /// `AppHandle` and knows its own data dir. This used to be a
    /// ~100-line near-duplicate of `ensure_started`; it is now a
    /// binding of the shared path like its Tauri counterpart, so a fix
    /// to spawn / teardown cannot land on one verb and miss the other.
    pub async fn restart_without_app_handle(
        &self,
        data_dir: &Path,
    ) -> Result<crate::dto::McpStatusDto, crate::error::AppError> {
        self.teardown(DaemonStatus::Restarting).await?;
        let _ = self
            .ensure_started_with(&data_dir.display().to_string())
            .await?
            .poll_only_finish();
        Ok(self.snapshot())
    }

    /// Is the daemon we are currently tracking still serving? The probe
    /// runs outside the lock (it is an HTTP round-trip), and the
    /// decision itself is the pure [`start_is_noop`].
    #[cfg(feature = "gui")]
    async fn is_serving(&self) -> bool {
        let (status, port) = {
            let g = self.inner.lock().await;
            (g.status, g.port)
        };
        let healthy = match port {
            Some(p) => port_registry::healthz(p).await,
            None => false,
        };
        start_is_noop(status, port, healthy)
    }

    /// Terminate whatever daemon we are tracking, then clear tracked
    /// state and drop the port file. Shared by `stop`, `restart`, and
    /// `shutdown`; `next` is the status the caller wants the snapshot
    /// to report while the teardown is in flight (`Stopped` for the
    /// manual switch and for app exit, `Restarting` for a restart).
    ///
    /// Ordering is load-bearing:
    ///
    ///   1. Resolve the kill targets **before** taking the lock. The
    ///      liveness probe costs a process spawn on Windows
    ///      (`tasklist`), and holding the supervisor mutex across it
    ///      would stall every status poll for the duration.
    ///   2. Under the lock: set the new status, then clear `pid` /
    ///      `port` / `port_record`, and take the child handle. Clearing
    ///      first is what makes the [`watcher_owns_state`] guard hold:
    ///      the exit-watcher woken by our own kill can no longer match,
    ///      so it cannot overwrite this status with `Crashed`.
    ///   3. Kill, then **wait (bounded) for the processes to actually
    ///      be gone** before releasing the port file. The old code
    ///      slept a flat 200ms and hoped; on Windows the child could
    ///      still be alive, and a socket that has not finished closing
    ///      keeps the port out of the bind queue elsewhere, so the
    ///      replacement could lose the port race and come up `Unhealthy`.
    async fn teardown(&self, next: DaemonStatus) -> Result<(), crate::error::AppError> {
        // (1) Resolve targets outside the lock. The port record is the
        // authority here: it survives a supervisor that never spawned
        // the daemon (the `serve-web` shape, a second restart in a row,
        // a fresh GUI adopting a daemon a previous one started).
        let record_pid = port_registry::read()
            .filter(|rec| port_registry::is_pid_alive(rec.pid))
            .map(|rec| rec.pid);
        let (child, targets) = {
            let mut g = self.inner.lock().await;
            let owned_pid = g.child.as_ref().and_then(|c| c.id());
            let targets = teardown_targets(owned_pid, record_pid);
            g.status = next;
            g.pid = None;
            g.port = None;
            g.port_record = None;
            (g.child.take(), targets)
        };

        // (3) Terminate. Owned children go first: we hold the handle,
        // so `try_wait` reaps them without an out-of-band signal.
        if let Some(mut child) = child {
            let _ = child.start_kill();
            if !wait_for_child_exit(&mut child, TEARDOWN_TIMEOUT).await {
                tracing::warn!(
                    target: "mcp_daemon",
                    "daemon pid {} still alive after {}ms — continuing anyway",
                    child.id().unwrap_or(0),
                    TEARDOWN_TIMEOUT.as_millis()
                );
            }
        }
        for target in targets.iter().filter(|t| !t.via_handle) {
            if !port_registry::terminate_pid(target.pid) {
                // Already gone, or the OS refused. Either way the wait
                // below is the real answer, so don't fail the verb.
                tracing::debug!(
                    target: "mcp_daemon",
                    "taskkill/kill -9 declined for adopted pid {}",
                    target.pid
                );
                continue;
            }
            if !wait_for_pid_exit(target.pid, TEARDOWN_TIMEOUT).await {
                tracing::warn!(
                    target: "mcp_daemon",
                    "adopted daemon pid {} still alive after {}ms — continuing anyway",
                    target.pid,
                    TEARDOWN_TIMEOUT.as_millis()
                );
            }
        }
        port_registry::release()
    }

    /// Best-effort shutdown. Called from the GUI's exit hook (and from
    /// tests). Shares the lifecycle teardown with the manual stop
    /// switch, so app exit also terminates an *adopted* daemon rather
    /// than orphaning it with its port file already gone.
    pub async fn shutdown(&self) -> Result<(), crate::error::AppError> {
        self.teardown(DaemonStatus::Stopped).await
    }

    /// The listener configuration to spawn with: the persisted row when
    /// this handle carries a pool, otherwise the shipped defaults.
    ///
    /// Read at *spawn* time rather than construction, so saving new
    /// settings and then hitting restart is enough — no re-creation of
    /// the managed state needed.
    ///
    /// **Fail-closed on a bad row.** An unreadable `mcp.http_config`
    /// resolves to [`McpHttpConfig::default`] — loopback-only — rather
    /// than propagating the error and refusing to start. A hand-edited
    /// or truncated row must not be able to brick the daemon, and the
    /// default is the direction that exposes nothing.
    fn http_config(&self) -> McpHttpConfig {
        let Some(pool) = self.pool.as_ref() else {
            return McpHttpConfig::default();
        };
        match crate::db::with_conn(pool, crate::db::preferences::mcp_http_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "mcp_daemon",
                    "mcp.http_config unreadable ({e:?}) — spawning on the loopback-only default"
                );
                McpHttpConfig::default()
            }
        }
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
#[cfg(feature = "gui")]
async fn watch_exit(inner: Arc<Mutex<DaemonState>>, app_handle: AppHandle, pid: u32) {
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
        // restart would have replaced both, and — since `teardown`
        // clears `pid` before killing — even a watcher woken by a
        // deliberate teardown sees `None` here and stays quiet.
        if watcher_owns_state(g.pid, pid) {
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

/// Web-shape counterpart of [`Self::watch_exit`]. Same F-D1 fix (take
/// the Child out of state under the lock, drop the guard, then await
/// `wait()`); the only difference is the missing `AppHandle` event
/// emission — the web shape has no Tauri bus, and the `/api/mcp/status`
/// poll covers the same surface.
async fn watch_exit_no_app(inner: Arc<Mutex<DaemonState>>, pid: u32) {
    let mut child = {
        let mut g = inner.lock().await;
        match g.child.take() {
            Some(c) => c,
            None => return,
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
    {
        let mut g = inner.lock().await;
        if watcher_owns_state(g.pid, pid) {
            g.status = DaemonStatus::Crashed { exit_code };
            g.last_error = Some(message);
            g.pid = None;
            g.port = None;
            g.port_record = None;
        }
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

/// Pure-logic tests for the lifecycle decisions (kill-target selection,
/// start idempotency, the exit-watcher guard) live in the sibling file
/// so they can never touch the process table or the real port file.
#[cfg(test)]
#[path = "daemon_supervisor_test.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_cli_args_use_real_http_transport_flag() {
        assert_eq!(
            daemon_cli_args(43123, &McpHttpConfig::default()),
            vec![
                "mcp",
                "--transport=http",
                "--port=43123",
                "--bind=127.0.0.1",
                "--path=/mcp",
                "--allowed-hosts=",
                "--allowed-origins=",
            ]
        );
    }

    #[test]
    fn daemon_cli_args_carry_the_persisted_listener_configuration() {
        // The child's CLI has to match the row the settings panel wrote,
        // otherwise the panel shows a LAN daemon that is still bound to
        // loopback (or the reverse).
        let cfg = McpHttpConfig {
            host: "0.0.0.0".into(),
            path: "/ai/mcp/".into(),
            allow_remote: true,
            allowed_hosts: vec!["studio.local".into(), "10.0.0.5".into()],
            allowed_origins: vec!["https://claude.ai".into()],
            ..McpHttpConfig::default()
        };
        let args = daemon_cli_args(4173, &cfg);
        for expected in [
            "--bind=0.0.0.0",
            "--path=/ai/mcp", // normalised: no trailing slash
            "--allow-remote",
            "--allowed-hosts=studio.local,10.0.0.5",
            "--allowed-origins=https://claude.ai",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "missing {expected} in {args:?}"
            );
        }
    }

    #[test]
    fn daemon_cli_args_omit_the_remote_flag_when_it_is_off() {
        // `--allow-remote` is a flag, so passing it while the setting is
        // false would open the daemon the user explicitly left closed.
        let args = daemon_cli_args(1, &McpHttpConfig::default());
        assert!(!args.iter().any(|a| a == "--allow-remote"), "got {args:?}");
    }

    #[test]
    fn daemon_cli_args_pass_empty_lists_explicitly() {
        // An empty host allowlist with `--allow-remote` is the daemon
        // *denying* every non-loopback Host, which is not the same as
        // "clap defaulted it" — the supervisor must not depend on that
        // coincidence.
        let cfg = McpHttpConfig {
            allow_remote: true,
            ..McpHttpConfig::default()
        };
        let args = daemon_cli_args(1, &cfg);
        assert!(args.iter().any(|a| a == "--allow-remote"));
        assert!(args.iter().any(|a| a == "--allowed-hosts="));
        assert!(args.iter().any(|a| a == "--allowed-origins="));
    }

    #[test]
    fn new_handle_snapshot_is_stopped() {
        let h = McpDaemonHandle::new();
        let s = h.snapshot();
        assert!(!s.running, "new handle should be stopped");
        assert_eq!(s.pid, None);
        assert_eq!(s.port, None);
        assert_eq!(s.requests_handled, 0);
        // Not `assert_eq!(.., 0)`: `McpMetrics::new()` stamps
        // `started_at_ms = now_ms()` and `snapshot()` returns the delta, so
        // a millisecond boundary crossed between the two lines makes this
        // read 1. It is a wall-clock difference between two adjacent
        // statements, not a state change — asserting exactly 0 reddened
        // CI at random whenever the machine was busy enough to deschedule
        // the thread in between.
        assert!(
            s.uptime_ms <= 1,
            "uptime of a handle that was never started should be ~0, got {}",
            s.uptime_ms
        );
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
        let v = serde_json::to_value(s).unwrap();
        let obj = v.as_object().expect("struct variant serializes to object");
        assert!(obj.contains_key("crashed"), "got: {v}");
        let inner = &obj["crashed"];
        assert_eq!(inner["exit_code"], 1);
    }

    #[test]
    fn crashed_with_no_exit_code_uses_null() {
        let s = DaemonStatus::Crashed { exit_code: None };
        let v = serde_json::to_value(s).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj["crashed"]["exit_code"].is_null());
    }
}
