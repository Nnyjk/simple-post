//! 日志导出的编排层。
//!
//! Tauri 命令与 REST 路由都只调这里，故两条 transport 拿到同一份文本 —— 见 AGENTS.md
//! 的 dual-transport invariant。日志本身住在 core 的 [`simple_post_core::logbuf`]。
//!
//! 日志可能含凭据与内部地址，故两条路由都**不在** `auth::is_public_path` 名单里：口令
//! 模式下导出必须带 bearer。

use serde::Serialize;

use simple_post_core::error::AppError;
use simple_post_core::logbuf::{self, LogLevel};

/// 单条前端消息（正文与 detail 各自）的字符上限。渲染进程的报告走同一条 IPC，不设上限
/// 时一次 `JSON.stringify` 的整份状态能把缓冲区挤掉。
pub const CLIENT_FIELD_LIMIT_CHARS: usize = 4 * 1024;

/// 「导出日志」的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExport {
    /// 建议的文件名：`simple-post-logs-<YYYYMMDD-HHMMSS>.txt` 或
    /// `simple-post-errors-<…>.txt`。
    pub file_name: String,
    /// 纯文本，一行一条记录。本次运行没有匹配的日志时为空串。
    pub content: String,
}

/// 导出范围。`All` 是缺省值；`Errors` 只留 `Warn` 及以上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Errors,
}

impl Scope {
    /// 只认 `"all"` / `"errors"`。其余值是调用方写错了 —— 静默按 `all` 处理会让「导出
    /// 错误日志」拿到一份含全部级别的文件，而调用方无从发现。
    fn parse(raw: &str) -> Result<Scope, AppError> {
        match raw {
            "all" => Ok(Scope::All),
            "errors" => Ok(Scope::Errors),
            other => Err(AppError::Validation(format!(
                "unknown log scope `{other}`: expected `all` or `errors`"
            ))),
        }
    }

    fn errors_only(self) -> bool {
        matches!(self, Scope::Errors)
    }

    fn file_prefix(self) -> &'static str {
        match self {
            Scope::All => "simple-post-logs",
            Scope::Errors => "simple-post-errors",
        }
    }
}

/// 取一份导出。`scope` 只接受 `"all"` / `"errors"`，调用方缺省时传 `"all"`。
pub fn export(scope: &str) -> Result<LogExport, AppError> {
    let scope = Scope::parse(scope)?;
    Ok(LogExport {
        file_name: format!("{}-{}.txt", scope.file_prefix(), logbuf::now_stamp()),
        content: logbuf::render(scope.errors_only()),
    })
}

/// 记一条渲染进程上报的事件。
///
/// 级别大小写不敏感，认不出按 `Info` 处理：日志写入不能因为级别拼错就失败，否则那次
/// 上报本身就从导出里消失了。`detail` 非空时另起一行接在正文之后。
pub fn append_client_event(
    level: &str,
    source: &str,
    message: &str,
    detail: Option<&str>,
) -> Result<(), AppError> {
    let level = LogLevel::from_label(level);
    let mut body = clamp_chars(message, CLIENT_FIELD_LIMIT_CHARS);
    if let Some(detail) = detail.map(str::trim).filter(|d| !d.is_empty()) {
        body.push('\n');
        body.push_str(&clamp_chars(detail, CLIENT_FIELD_LIMIT_CHARS));
    }
    logbuf::record(level, &format!("webview:{source}"), body);
    Ok(())
}

/// 按字符（不是字节）截断，故多字节字符不会被切坏。
fn clamp_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

// 测试——范围取值、文件名与前端上报的正文形状。

#[cfg(test)]
#[path = "log_api_test.rs"]
mod tests;
