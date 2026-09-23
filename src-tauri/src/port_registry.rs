//! Port registry for the MCP HTTP daemon.
//!
//! The daemon writes a small JSON record (`mcp.port`) to the platform
//! data directory on startup. The GUI reads it on launch to discover
//! the daemon endpoint, and the standalone `simple-post-mcp-proxy`
//! binary reads the same file to know where to forward JSON-RPC traffic
//! over stdio.
//!
//! Concurrency: two GUI instances starting at the same time both try to
//! claim the file. We serialize the write path through a process-wide
//! `tokio::sync::Mutex` and use `O_EXCL` (`OpenOptions::create_new`) so
//! the OS itself rejects the second writer. The loser re-reads, finds a
//! live PID, and surfaces a `Validation` error rather than silently
//! stomping on the winner's record.
//!
//! File path:
//!   * Windows: `%APPDATA%\simple-post\mcp.port`
//!   * macOS:   `~/Library/Application Support/simple-post/mcp.port`
//!   * Linux:   `$XDG_DATA_HOME/simple-post/mcp.port` (or
//!     `~/.local/share/simple-post/mcp.port`)
//!
//! For tests we honor `SIMPLE_POST_DATA_DIR` as the base directory so
//! the unit tests can avoid clobbering a real user's port file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Name of the registry file inside the data directory. Exposed so the
/// proxy binary can open the same path without re-deriving the layout.
pub const FILE_NAME: &str = "mcp.port";

/// Environment variable that overrides the data dir for tests. When
/// set, `port_file_path()` uses `$SIMPLE_POST_DATA_DIR/simple-post/mcp.port`
/// instead of the platform default.
const DATA_DIR_ENV: &str = "SIMPLE_POST_DATA_DIR";

/// What the daemon wrote on startup. The renderer never sees this; the
/// proxy binary reads just enough of it to learn `port` and forwards
/// everything else.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortRecord {
    /// PID of the daemon process. 0 means "not yet known" (used briefly
    /// between file write and `child.id()` round-trip).
    pub pid: u32,
    /// TCP port the daemon is listening on (loopback).
    pub port: u16,
    /// Bind interface, e.g. `127.0.0.1`. The proxy uses this verbatim.
    pub bind: String,
    /// Unix epoch seconds when the daemon wrote this record. Lets the
    /// GUI / proxy diagnose "this port file is 12 hours old, the daemon
    /// probably died" without external state.
    pub started_at: u64,
    /// Absolute data directory the daemon was serving. Kept for
    /// diagnostics and so the GUI can sanity-check it matches the
    /// instance that wrote the file.
    pub data_dir: String,
    /// Build version of the writing binary (`CARGO_PKG_VERSION`).
    /// Helpful when chasing "old daemon still running" support tickets.
    pub version: String,
}

impl PortRecord {
    /// Build a record from the daemon's own metadata. `pid == 0` is fine
    /// for the first write; `ensure_started` rewrites once `child.id()`
    /// is known.
    pub fn new(pid: u32, port: u16, bind: impl Into<String>, data_dir: impl Into<String>) -> Self {
        Self {
            pid,
            port,
            bind: bind.into(),
            started_at: now_secs(),
            data_dir: data_dir.into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Global write lock so two GUI instances in the same process can't
/// race. Held across `create_new` + the read-existing fallback so the
/// check-and-write is atomic from the perspective of other writers.
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn write_lock() -> &'static Mutex<()> {
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// Resolve the base directory: env override → `dirs::data_dir()`. The
/// returned path is the parent that will contain `simple-post/mcp.port`.
fn base_dir() -> Result<PathBuf, crate::error::AppError> {
    if let Some(s) = std::env::var_os(DATA_DIR_ENV) {
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    dirs::data_dir().ok_or_else(|| {
        crate::error::AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "platform data_dir is unavailable",
        ))
    })
}

/// Public: the directory the daemon writes its data into (and where the
/// `mcp.port` file lives). Public so the daemon + GUI can open the
/// SQLite pool under the same path the proxy will discover.
///
/// When the `SIMPLE_POST_DATA_DIR` env override is set, this returns
/// it verbatim (the SQLite file and the `mcp.port` file both end up
/// directly under the override directory — flat layout for tests +
/// power users). When not set, falls back to `dirs::data_dir()` with
/// no `com.simple-post` segment — callers that want the historical
/// `com.simple-post` suffix can `.join("com.simple-post")` themselves.
pub fn data_dir() -> Result<PathBuf, crate::error::AppError> {
    base_dir()
}

/// Internal: compute the port-file path under an arbitrary base. Used
/// by tests that don't want to touch the user's real data dir.
fn port_file_path_at(base: &Path) -> Result<PathBuf, crate::error::AppError> {
    let dir = base.join("simple-post");
    std::fs::create_dir_all(&dir).map_err(crate::error::AppError::Io)?;
    Ok(dir.join(FILE_NAME))
}

/// Absolute path to `mcp.port`. Creates the `simple-post` parent
/// directory if it doesn't exist yet (idempotent — the daemon process
/// is the first writer on a fresh install).
pub fn port_file_path() -> Result<PathBuf, crate::error::AppError> {
    port_file_path_at(&base_dir()?)
}

/// Read the current record. Returns `None` if the file doesn't exist,
/// isn't readable, or fails to deserialize (corrupt/partial write —
/// treated as "no daemon", safer than panicking).
pub fn read() -> Option<PortRecord> {
    let path = port_file_path().ok()?;
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice::<PortRecord>(&bytes) {
        Ok(rec) => Some(rec),
        Err(e) => {
            tracing::debug!(
                target: "port_registry",
                "mcp.port present but unparseable, treating as missing: {e}"
            );
            None
        }
    }
}

/// Best-effort PID liveness check.
///
///   * Windows — spawns `tasklist /FI "PID eq <pid>"` and looks for the
///     CSV row containing the PID. `tasklist` exits 0 with output even
///     when the PID doesn't exist (it prints "INFO: No tasks …"), so
///     the only reliable signal is the presence of the PID in stdout.
///
///   * Unix — spawns `kill -0 <pid>` and checks exit code. Exit 0 means
///     the process exists (even if owned by another user). No new
///     crates required; `kill` is in coreutils on every supported
///     platform. We chose std::process over `libc::kill` to avoid
///     pulling in a second direct dep just for one syscall.
///
/// Both backends are best-effort: a TOCTOU between this check and the
/// caller acting on the result is unavoidable without holding a kernel
/// handle. The caller (`write`) treats "PID says alive" as "another
/// daemon owns the file; refuse"; treating "PID says dead" as
/// "unlink and retry" — so the worst case is one extra restart, not
/// data loss.
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let needle = format!("\"{pid}\"");
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout);
                // tasklist prints "INFO: No tasks are running..." for a
                // missing PID — that line doesn't contain our quoted
                // needle, so the membership test is sufficient.
                s.contains(&needle)
            }
            Err(_) => false,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Try one create_new write. Returns:
///   * `Ok(true)`  — wrote successfully.
///   * `Ok(false)` — file already existed (`AlreadyExists`), caller should
///     inspect the existing record.
///   * `Err(_)`    — actual IO failure.
fn try_create_new(path: &Path, json: &[u8]) -> Result<bool, crate::error::AppError> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            f.write_all(json)?;
            f.sync_all()?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(crate::error::AppError::Io(e)),
    }
}

/// Acquire the registry file for `rec`.
///
/// Behavior:
///   1. Try `create_new(true)` (atomic O_EXCL on Unix, equivalent on
///      Windows). On success, write and return.
///   2. If the file already exists, read the existing record:
///      - Live PID → `Validation("port in use by live pid X")`. Caller
///        surfaces this to the user; we don't stomp.
///      - Dead PID (or unreadable) → remove and retry, up to 3 times.
///        Three retries shields us from a TOCTOU where another writer
///        repopulates the file between our `remove` and `create_new`.
///   3. Other IO errors bubble up as `AppError::Io`.
///
/// The function is synchronous because the caller (daemon supervisor)
/// already sits inside a tokio task and we don't want to drag the
/// runtime's `block_on` into a second layer. The shared mutex is a
/// `tokio::sync::Mutex` because that's what's available without a
/// second crate; we never await it from async context here.
pub fn write(rec: &PortRecord) -> Result<(), crate::error::AppError> {
    let json = serde_json::to_vec(rec)?;

    let lock = write_lock();
    let guard = match tokio::runtime::Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(lock.lock())),
        Err(_) => {
            // No runtime yet (unit-test path). Build a temporary
            // single-thread runtime just to take the lock.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(crate::error::AppError::Io)?;
            rt.block_on(lock.lock())
        }
    };
    let _guard = guard;

    for _attempt in 0..3 {
        let path = port_file_path()?;
        if try_create_new(&path, &json)? {
            return Ok(());
        }
        // File is there. Inspect it.
        let existing = read();
        if let Some(prev) = existing {
            if is_pid_alive(prev.pid) {
                return Err(crate::error::AppError::Validation(format!(
                    "port in use by live pid {}",
                    prev.pid
                )));
            }
            tracing::info!(
                target: "port_registry",
                "stale mcp.port (pid {} dead) — removing and retrying",
                prev.pid
            );
        }
        // Best-effort cleanup; ignore NotFound. If removal fails for any
        // other reason we keep retrying — maybe next round will succeed.
        match std::fs::remove_file(port_file_path()?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(target: "port_registry", "remove_file: {e}");
            }
        }
    }

    Err(crate::error::AppError::Validation(
        "failed to acquire port file after 3 retries".into(),
    ))
}

/// Remove the port file (called on GUI shutdown).
/// Idempotent — `NotFound` is treated as success so the renderer can
/// call this freely without `if path.exists()` guards.
pub fn release() -> Result<(), crate::error::AppError> {
    let path = port_file_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::AppError::Io(e)),
    }
}

/// HTTP probe used by the GUI to detect a daemon that crashed but left
/// its port file behind. 2-second timeout — long enough to span a
/// busy-CPU moment on startup, short enough that the renderer doesn't
/// visibly stall on the splash.
pub async fn healthz(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/mcp/healthz");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "port_registry", "healthz client build: {e}");
            return false;
        }
    };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(e) => {
            tracing::debug!(target: "port_registry", "healthz {url}: {e}");
            false
        }
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Process-wide mutex that serializes any test which touches the
    /// `SIMPLE_POST_DATA_DIR` env var. The env var is a single global slot
    /// shared by every thread in the process, so without this lock two
    /// parallel tests would race: one sets the var, the other writes under
    /// that path, the first releases — and the read the second sees
    /// belongs to a completely different scratch dir.
    ///
    /// We deliberately hold the guard for the *whole* test (including the
    /// `release()`/`read()` calls inside it) by tying it to the
    /// `ScopedEnv` RAII handle. Restoring the previous env var on drop
    /// thus also happens before the next test acquires the lock.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Each test gets its own tempdir under the OS temp dir, then points
    /// `SIMPLE_POST_DATA_DIR` at it so `port_file_path()` resolves under
    /// the scratch instead of touching the real APPDATA / XDG_DATA_HOME.
    /// Returns the parent; tests are responsible for removing it on
    /// success (cleanup runs anyway on best-effort).
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct ScopedEnv {
        _guard: std::sync::MutexGuard<'static, ()>,
        prev: Option<std::ffi::OsString>,
        scratch: PathBuf,
    }

    impl ScopedEnv {
        fn new() -> Self {
            // Acquire the process-wide test lock first, *then* swap the
            // env var. Order matters: if another test is mid-flight when
            // we swap, our writes would land in its scratch.
            let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let scratch = std::env::temp_dir().join(format!(
                "simple-post-port-test-{}-{id}-{}",
                std::process::id(),
                now_secs()
            ));
            std::fs::create_dir_all(&scratch).unwrap();
            let prev = std::env::var_os(DATA_DIR_ENV);
            // SAFETY: the lock above guarantees no other thread observes
            // a partial state — we set the var, run the test body, and
            // restore it on drop, all under the held guard.
            unsafe { std::env::set_var(DATA_DIR_ENV, &scratch) };
            Self {
                _guard: guard,
                prev,
                scratch,
            }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(DATA_DIR_ENV, v) },
                None => unsafe { std::env::remove_var(DATA_DIR_ENV) },
            }
            let _ = std::fs::remove_dir_all(&self.scratch);
            // `_guard` is dropped after this method returns, releasing
            // the mutex for the next test.
        }
    }

    fn sample(port: u16, pid: u32) -> PortRecord {
        PortRecord::new(
            pid,
            port,
            "127.0.0.1",
            std::env::temp_dir().display().to_string(),
        )
    }

    #[test]
    fn write_then_read_returns_same_record() {
        let _env = ScopedEnv::new();
        // pid 0 short-circuits is_pid_alive, so we don't risk
        // tripping over our own test runner process.
        let rec = sample(41234, 0);
        write(&rec).expect("write");
        let got = read().expect("read after write");
        assert_eq!(got, rec);
        release().unwrap();
    }

    #[test]
    fn write_then_release_then_read_is_none() {
        let _env = ScopedEnv::new();
        let rec = sample(41235, 0);
        write(&rec).expect("write");
        assert!(read().is_some(), "should be readable before release");
        release().expect("release");
        assert!(read().is_none(), "should be gone after release");
    }

    #[test]
    fn write_with_live_existing_pid_rejects() {
        let _env = ScopedEnv::new();
        // First writer: pid = our own process id (definitely alive).
        let rec1 = sample(41236, std::process::id());
        write(&rec1).expect("first write");
        // Second writer: pid 0, different port — should be rejected.
        let rec2 = sample(41237, 0);
        let err = write(&rec2).expect_err("second write should fail");
        match err {
            crate::error::AppError::Validation(msg) => {
                assert!(msg.contains("live pid"), "msg: {msg}");
                assert!(msg.contains(&std::process::id().to_string()), "msg: {msg}");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
        release().unwrap();
    }

    #[test]
    fn write_replaces_stale_record_after_dead_pid() {
        let _env = ScopedEnv::new();
        // First writer: pid 999999999 — almost certainly not alive.
        // is_pid_alive will return false for any PID we don't have, so
        // we don't need to rely on the OS agreeing.
        let rec1 = sample(41238, 999_999_999);
        write(&rec1).expect("first write");
        // Second writer: fresh record, different port. The stale record
        // should be detected and replaced.
        let rec2 = sample(41239, 0);
        write(&rec2).expect("second write replaces stale");
        let got = read().expect("read after replace");
        assert_eq!(got.port, 41239);
        assert_eq!(got.pid, 0);
        release().unwrap();
    }

    #[test]
    fn release_is_idempotent_when_missing() {
        let _env = ScopedEnv::new();
        // No prior write.
        release().expect("release on empty dir");
        release().expect("release again");
    }

    #[test]
    fn is_pid_alive_zero_is_false() {
        assert!(!is_pid_alive(0));
    }

    #[test]
    fn is_pid_alive_self_is_true() {
        // Both Windows (windows-sys) and Unix (kill -0) backends
        // return true for the running test process.
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn is_pid_alive_far_far_future_pid_is_false() {
        // Pick a pid far outside any plausible OS range; the OS will
        // say it doesn't exist. We don't hardcode a value because
        // pid_t is u32 on Windows but i32 on Unix — but u32::MAX / 2
        // is safely unrepresentable on both.
        let pid = u32::MAX / 2;
        assert!(!is_pid_alive(pid));
    }

    #[test]
    fn record_carries_version_and_epoch() {
        let before = now_secs();
        let rec = sample(41240, 0);
        let after = now_secs();
        assert_eq!(rec.version, env!("CARGO_PKG_VERSION"));
        assert!(rec.started_at >= before && rec.started_at <= after);
        assert_eq!(rec.bind, "127.0.0.1");
    }

    #[test]
    fn corrupt_file_is_treated_as_missing() {
        let _env = ScopedEnv::new();
        let path = port_file_path().unwrap();
        std::fs::write(&path, b"{not really json").unwrap();
        assert!(read().is_none(), "garbage in file ⇒ read returns None");
        let _ = std::fs::remove_file(&path);
    }
}
