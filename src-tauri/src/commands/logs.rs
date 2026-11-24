//! 日志导出的 Tauri 入口。
//!
//! 三个命令都是薄壳，逻辑在 [`crate::log_api`]（server crate）；REST 侧的
//! `GET /api/logs/export` 与 `POST /api/logs/client` 调的是同一批函数，故两条 transport
//! 拿到同一份文本。
//!
//! **没有口令判据**：`serve-web` 的 bearer 闸在路由层，桌面壳是本机 IPC 调用，没有 bearer。
//!
//! 操作系统交互只住在这里：`log_api` 只管写文本，打开文件管理器是桌面壳的事。

use std::path::Path;

use crate::error::AppError;
use crate::log_api::{LogExport, LogExportFile};

/// 导出本次运行期间的日志。
///
/// `scope` 只接受 `"all"` / `"errors"`，缺省按 `"all"`。返回的是**内容**而不是路径：
/// 桌面侧由前端弹保存对话框（`saveGeneratedDocument`），浏览器侧同理——写盘位置由用户
/// 定，进程不去猜。
#[tauri::command]
pub fn log_export(scope: Option<String>) -> Result<LogExport, AppError> {
    crate::log_api::export(scope.as_deref().unwrap_or("all"))
}

/// 导出到本机日志目录，并在文件管理器里选中那个文件。
///
/// 返回 `None` 表示没有可导出的日志，此时不创建文件、也不打开文件管理器。定位失败
/// 不算导出失败：文件已经写好了，打开文件夹只省用户一次手点。
#[tauri::command]
pub fn log_export_file(scope: Option<String>) -> Result<Option<LogExportFile>, AppError> {
    let file = crate::log_api::export_to_disk(scope.as_deref().unwrap_or("all"))?;
    if let Some(file) = &file {
        if let Err(e) = reveal(Path::new(&file.path)) {
            // 本 crate 的诊断一律走 stderr（与 `mcp_status` 等处一致）：这是桌面壳，
            // 手上没有 `tracing` 订阅者。
            eprintln!("[log_export_file] 打开日志所在目录失败: {e:?}");
        }
    }
    Ok(file)
}

/// Windows：`explorer /select,"<path>"`。`raw_arg` 原样传参，路径含空格也认；
/// `explorer` 即便成功也常返回非零退出码，故只看能否启动。
#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
}

/// macOS：`open -R <path>` 就是在访达里选中该文件。
#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
}

/// 其余 unix：`xdg-open` 没有「选中文件」的通用做法，打开所在目录即可。
#[cfg(all(unix, not(target_os = "macos")))]
fn reveal(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn()
        .map(|_| ())
}

/// 渲染进程上报一条事件（`console.error` / 未捕获异常 / 未处理的 reject）。
///
/// 级别拼错不失败：写入侧按 `Info` 处理，否则那条上报本身会从导出里消失。
#[tauri::command]
pub fn log_client_event(
    level: String,
    source: String,
    message: String,
    detail: Option<String>,
) -> Result<(), AppError> {
    crate::log_api::append_client_event(&level, &source, &message, detail.as_deref())
}
