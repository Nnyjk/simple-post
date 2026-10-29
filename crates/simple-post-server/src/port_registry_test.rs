//! 端口注册表的文件级测试。
//!
//! `port_registry.rs` 的同级 `#[path]` 模块（以 `mod tests` 挂载）。全部打到
//! `SIMPLE_POST_DATA_DIR` 指向的系统临时目录，永不触碰开发者真实的 `mcp.port`。

use super::*;
// 这两个住在子模块里，本文件从那里取。
use super::paths::DATA_DIR_ENV;
use super::record::now_secs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// 串行化任何碰 `SIMPLE_POST_DATA_DIR` 环境变量的测试的进程级 mutex。该环境变量
/// 是全进程共享的单个槽位，没有这把锁两个并行测试就会互相踩：一个刚设好变量，
/// 另一个已在那个路径下写入，第一个随后释放——第二个读到的属于一个完全不同的
/// scratch 目录。
///
/// 刻意把 guard 持有*整个*测试体（包括其中的 `release()` / `read()`），做法是
/// 把它绑到 `ScopedEnv` 这个 RAII 句柄上。于是 drop 时恢复上一个环境变量也发生在
/// 下一个测试拿到锁之前。
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 每个测试在系统临时目录下拿到自己的临时目录，再把 `SIMPLE_POST_DATA_DIR`
/// 指过去，使 `port_file_path()` 落在 scratch 而不是真实的 APPDATA /
/// XDG_DATA_HOME 上。
static COUNTER: AtomicU32 = AtomicU32::new(0);

struct ScopedEnv {
    _guard: std::sync::MutexGuard<'static, ()>,
    prev: Option<std::ffi::OsString>,
    scratch: PathBuf,
}

impl ScopedEnv {
    fn new() -> Self {
        // 先拿进程级测试锁，*再*换环境变量。顺序有讲究：若另一个测试正在跑而我们换了
        // 变量，我们的写会落到它的 scratch 里。
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "simple-post-port-test-{}-{id}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let prev = std::env::var_os(DATA_DIR_ENV);
        // SAFETY：上面的锁保证没有别的线程看到中间状态——设变量、跑测试体、drop 时恢复，
        // 全程都在持有的 guard 之内。
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
        // `_guard` 在本方法返回后才 drop，把 mutex 让给下一个测试。
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
    // pid 0 会让 `is_pid_alive` 短路，免得撞上测试运行器进程自身。
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
    // 第一个写入者：pid 取我们自己的进程 id（肯定活着）。
    let rec1 = sample(41236, std::process::id());
    write(&rec1).expect("first write");
    // 第二个写入者：pid 0，端口不同——应被拒。
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
    // 第一个写入者：pid 999999999——几乎肯定不存在。`is_pid_alive` 对任何我们没有的
    // PID 都返回 false，故不必依赖 OS 的具体说法。
    let rec1 = sample(41238, 999_999_999);
    write(&rec1).expect("first write");
    // 第二个写入者：全新记录、端口不同。陈旧记录应被检出并替换。
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
    // 这个目录里还没有写入过任何记录。
    release().expect("release on empty dir");
    release().expect("release again");
}

#[test]
fn is_pid_alive_zero_is_false() {
    assert!(!is_pid_alive(0));
}

#[test]
fn is_pid_alive_self_is_true() {
    // Windows（windows-sys）与 Unix（kill -0）两个后端对本进程都返回 true。
    assert!(is_pid_alive(std::process::id()));
}

#[test]
fn is_pid_alive_far_far_future_pid_is_false() {
    // 取一个远超任何合理 OS 范围的 pid。不写死具体值，因为 pid_t 在 Windows 上是
    // u32、在 Unix 上是 i32——但 `u32::MAX / 2` 在两者上都不可表示。
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
