//! 日志导出的 Tauri 入口。
//!
//! 两个命令都是薄壳，逻辑在 [`crate::log_api`]（server crate）；REST 侧的
//! `GET /api/logs/export` 与 `POST /api/logs/client` 调的是同一批函数，故两条 transport
//! 拿到同一份文本。
//!
//! **没有口令判据**：`serve-web` 的 bearer 闸在路由层，桌面壳是本机 IPC 调用，没有 bearer。

use crate::error::AppError;
use crate::log_api::LogExport;

/// 导出本次运行期间的日志。
///
/// `scope` 只接受 `"all"` / `"errors"`，缺省按 `"all"`。返回的是**内容**而不是路径：
/// 桌面侧由前端弹保存对话框（`saveGeneratedDocument`），浏览器侧同理——写盘位置由用户
/// 定，进程不去猜。
#[tauri::command]
pub fn log_export(scope: Option<String>) -> Result<LogExport, AppError> {
    crate::log_api::export(scope.as_deref().unwrap_or("all"))
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
