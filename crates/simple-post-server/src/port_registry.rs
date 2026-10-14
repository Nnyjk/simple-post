//! MCP HTTP daemon 的端口注册表。
//!
//! daemon 启动时把一条小 JSON 记录（`mcp.port`）写到平台数据目录。GUI 启动时读它
//! 以发现 daemon 端点；独立的 `simple-post-mcp-proxy` 二进制读同一个文件，得知把
//! JSON-RPC 流量转到哪里。
//!
//! 并发：两个 GUI 实例同时启动都会抢这个文件。写路径用进程级
//! `tokio::sync::Mutex` 串起来，并用 `O_EXCL`（`OpenOptions::create_new`）让 OS
//! 自己拒掉第二个写入者。输家重读、发现是个活 PID，于是报 `Validation` 错误，而不是
//! 悄悄覆盖赢家的记录。
//!
//! 文件路径：
//!   * Windows: `%APPDATA%\simple-post\mcp.port`
//!   * macOS:   `~/Library/Application Support/simple-post/mcp.port`
//!   * Linux:   `$XDG_DATA_HOME/simple-post/mcp.port`（或
//!     `~/.local/share/simple-post/mcp.port`）
//!
//! 测试下用 `SIMPLE_POST_DATA_DIR` 覆盖基准目录，免得单测踩掉真实用户的端口文件。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// 数据目录内的注册文件名。导出，好让 proxy 二进制不必重新推导目录结构就能打开
/// 同一路径。
pub const FILE_NAME: &str = "mcp.port";

/// 测试用数据目录覆盖环境变量。设置后 `port_file_path()` 用
/// `$SIMPLE_POST_DATA_DIR/simple-post/mcp.port` 而非平台默认。
const DATA_DIR_ENV: &str = "SIMPLE_POST_DATA_DIR";

/// daemon 启动时写入的内容。renderer 从不看到它；proxy 二进制只取其中的 `port`，
/// 其余原样转发。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortRecord {
    /// daemon 进程 PID。0 表示「尚不知道」（文件写入与 `child.id()` 往返之间会
    /// 短暂出现）。
    pub pid: u32,
    /// daemon 正在监听的 TCP 端口。
    pub port: u16,
    /// 绑定接口，如 `127.0.0.1`。proxy 原样使用。
    pub bind: String,
    /// daemon 写入本记录的 Unix epoch 秒。便于 GUI / proxy 在没有外部状态的情况下
    /// 判断「这个端口文件已经 12 小时了，daemon 大概死了」。
    pub started_at: u64,
    /// daemon 当时服务的数据目录绝对路径。仅供诊断，也让 GUI 能核对它与写入方
    /// 实例是否一致。
    pub data_dir: String,
    /// 写入方二进制的构建版本（`CARGO_PKG_VERSION`）。
    pub version: String,
}

impl PortRecord {
    /// 用 daemon 自己的元数据构造记录。首次写入时 `pid == 0` 可以接受；
    /// `ensure_started` 会在 `child.id()` 已知后重写一次。
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

/// 全局写锁，防止同进程内两个实例互相踩。持有范围覆盖 `create_new` 与「读现有
/// 记录再决定」的整个回落流程，使 check-and-write 对其他写入者是原子的。
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn write_lock() -> &'static Mutex<()> {
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 解析基准目录：环境变量覆盖 → `dirs::data_dir()`。返回的路径是容纳
/// `simple-post/mcp.port` 的父目录。
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

/// 公开：daemon 写数据的目录（也是 `mcp.port` 文件所在处）。公开是因为 daemon 与
/// GUI 需要在 proxy 将要发现的同一路径下打开 SQLite 池。
///
/// 设置 `SIMPLE_POST_DATA_DIR` 时原样返回它；否则回退 `dirs::data_dir()`，**不带**
/// `com.simple-post` 段——需要该后缀的调用方自己 `.join("com.simple-post")`。
pub fn data_dir() -> Result<PathBuf, crate::error::AppError> {
    base_dir()
}

/// 内部：在任意基准目录下算出端口文件路径。供不想碰用户真实数据目录的测试使用。
fn port_file_path_at(base: &Path) -> Result<PathBuf, crate::error::AppError> {
    let dir = base.join("simple-post");
    std::fs::create_dir_all(&dir).map_err(crate::error::AppError::Io)?;
    Ok(dir.join(FILE_NAME))
}

/// `mcp.port` 的绝对路径。必要时创建 `simple-post` 父目录（幂等——全新安装上第一个
/// 写它的就是 daemon 进程）。
pub fn port_file_path() -> Result<PathBuf, crate::error::AppError> {
    port_file_path_at(&base_dir()?)
}

/// 读当前记录。文件不存在、读不了或反序列化失败（损坏 / 写了一半——按「没有 daemon」
/// 处理，比 panic 安全）时返回 `None`。
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

/// 尽力而为的 PID 存活探测。
///
///   * Windows —— spawn `tasklist /FI "PID eq <pid>"`，在 CSV 行里找这个 PID。
///     PID 不存在时 `tasklist` 仍以 0 退出且有输出（打 "INFO: No tasks …"），故唯一
///     可靠的信号是该 PID 是否出现在 stdout 中。
///
///   * Unix —— spawn `kill -0 <pid>` 看退出码。退出 0 表示进程存在（哪怕属于别的
///     用户）。不引入新 crate：`kill` 在所有支持平台的 coreutils 里都有。选
///     `std::process` 而非 `libc::kill`，只为一个系统调用不值得多一个直接依赖。
///
/// 两个后端都是尽力而为：不持有内核句柄时，本检查与调用方据此行动之间的 TOCTOU
/// 无法避免。调用方（`write`）把「PID 说活着」当作「另一个 daemon 占着文件，拒绝」，
/// 把「PID 说死了」当作「unlink 后重试」——最坏结果是一次多余的重启，不是数据丢失。
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
                // PID 不存在时 tasklist 打的是 "INFO: No tasks are running..."，那行不含我们加引号的
                // needle，故成员判断即足够。
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

/// 强制终止 `PortRecord` 点名的 PID。
///
/// 这是 [`is_pid_alive`] 的带外对应物，且只有一个调用方：daemon supervisor 的
/// teardown。`ensure_started` 的复用路径会*收养*已在运行的 daemon（端口记录里是活
/// PID + healthz 绿）却从未 spawn 它，故监管者对该进程没有 `Child` 句柄，无法走
/// 常规 reap。只查自己手上那个句柄的重启会静默地重新收养它本该替换掉的进程；本函数
/// 才让这一种情况真的被终止。
///
///   * Windows —— `taskkill /F /PID <pid>`。强制（没有优雅窗口：daemon 是无状态的
///     JSON-RPC 循环，没什么要 flush），且**刻意不带** `/T`，这样被回收复用的 PID
///     最多让调用方损失自己的 daemon，绝不牵连无关进程树。
///   * Unix —— `kill -9 <pid>`。
///
/// 返回 OS 是否接受了该信号。`false` 是尽力而为下的噪声，不是契约违约：调用方会在
/// 有界等待里用 [`is_pid_alive`] 复核，而「进程本来就已经没了」对 teardown 而言完全
/// 是个可接受的结果。
pub fn terminate_pid(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// 试一次 create_new 写入。返回：
///   * `Ok(true)`  —— 写成功。
///   * `Ok(false)` —— 文件已存在（`AlreadyExists`），调用方应去查看现有记录。
///   * `Err(_)`    —— 真正的 IO 失败。
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

/// 申领 `rec` 的注册文件。
///
/// 行为：
///   1. 先试 `create_new(true)`（Unix 上的原子 O_EXCL，Windows 上等价）。成功则写入并
///      返回。
///   2. 文件已存在则读现有记录：
///      - PID 活着 → `Validation("port in use by live pid X")`，由调用方呈现给用户；
///        我们不覆盖。
///      - PID 已死（或读不出）→ 删掉重试，最多 3 次。三次重试挡住的是另一个写入者在
///        我们的 `remove` 与 `create_new` 之间又把文件填回去的 TOCTOU。
///   3. 其他 IO 错误以 `AppError::Io` 上抛。
///
/// 本函数是同步的，因为调用方（daemon supervisor）本就在 tokio 任务里，不想把运行时
/// 的 `block_on` 再拖进一层。共享锁是 `tokio::sync::Mutex`，因为不引第二个 crate 就
/// 只有它可用；此处从不从异步上下文 await 它。
pub fn write(rec: &PortRecord) -> Result<(), crate::error::AppError> {
    let json = serde_json::to_vec(rec)?;

    let lock = write_lock();
    let guard = match tokio::runtime::Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(lock.lock())),
        Err(_) => {
            // 还没有运行时（单测路径）：临时建一个单线程运行时，只为拿这把锁。
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
        // 文件在。检查它。
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
        // 尽力清理，NotFound 忽略。其他原因失败则继续重试——也许下一轮就成功。
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

/// 删掉端口文件（GUI 退出时调用）。幂等——`NotFound` 视作成功，故 renderer 可以
/// 随意调用而不必先 `if path.exists()`。
pub fn release() -> Result<(), crate::error::AppError> {
    let path = port_file_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::AppError::Io(e)),
    }
}

/// GUI 用来发现「崩溃但留下端口文件」的 daemon 的 HTTP 探测。2 秒超时——既能跨过
/// 启动时 CPU 繁忙的瞬间，又不至于让渲染进程在启动画面上明显卡住。
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
        // 此前没有写入。
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
}
