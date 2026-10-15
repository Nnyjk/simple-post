//! 多实例申领注册文件。这是 `mcp.port` 的唯一写入口，也是本文件里唯一必须整块移动的
//! 单元：`create_new`（OS 级 O_EXCL）与它的回落/报错逻辑合起来才构成「第二个 GUI
//! 抢不到文件」这条保证，拆开任何一半都会让它退化成先检查后写入。

use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

use tokio::sync::Mutex;

use super::paths::port_file_path;
use super::proc::is_pid_alive;
use super::read;
use crate::error::AppError;
use crate::port_registry::PortRecord;

/// 全局写锁，防止同进程内两个实例互相踩。持有范围覆盖 `create_new` 与「读现有
/// 记录再决定」的整个回落流程，使 check-and-write 对其他写入者是原子的。
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn write_lock() -> &'static Mutex<()> {
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

/// 试一次 create_new 写入。返回：
///   * `Ok(true)`  —— 写成功。
///   * `Ok(false)` —— 文件已存在（`AlreadyExists`），调用方应去查看现有记录。
///   * `Err(_)`    —— 真正的 IO 失败。
fn try_create_new(path: &Path, json: &[u8]) -> Result<bool, AppError> {
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
pub fn write(rec: &PortRecord) -> Result<(), AppError> {
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
