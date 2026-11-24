//! 日志导出的编排层。
//!
//! Tauri 命令与 REST 路由都只调这里，故两条 transport 拿到同一份文本 —— 见 AGENTS.md
//! 的 dual-transport invariant。日志本身住在 core 的 [`simple_post_core::logbuf`]。
//!
//! 日志可能含凭据与内部地址，故两条路由都**不在** `auth::is_public_path` 名单里：口令
//! 模式下导出必须带 bearer。
//!
//! 桌面端还有一条 [`export_to_disk`]：日志直接落在 `<data_dir>/simple-post/logs/`，
//! 由桌面壳打开所在文件夹。它写的是同一份文本，故同一条鉴权纪律——内容可能含凭据，
//! 这个入口只给本机 IPC 用，不挂到 REST 上。

use std::path::{Path, PathBuf};

use serde::Serialize;

use simple_post_core::error::AppError;
use simple_post_core::logbuf::{self, LogLevel};
use simple_post_core::port_file;

/// 单条前端消息（正文与 detail 各自）的字符上限。渲染进程的报告走同一条 IPC，不设上限
/// 时一次 `JSON.stringify` 的整份状态能把缓冲区挤掉。
pub const CLIENT_FIELD_LIMIT_CHARS: usize = 4 * 1024;

/// 「导出日志」的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExport {
    /// 建议的文件名：`simple-post-logs-<YYYYMMDD-HHMMSS>.txt` 或错误日志 `simple-post-errors-<…>.txt`。
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

/// 桌面端落盘的结果：文件名与完整路径。字段名与前端 `LogExportFile` 逐字一致。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExportFile {
    pub file_name: String,
    pub path: String,
}

/// 日志目录在任意基准目录下的位置。`base` 是平台数据目录，不带应用段。
pub fn log_dir_at(base: &Path) -> PathBuf {
    base.join("simple-post").join("logs")
}

/// 本机的日志目录，必要时创建。路径规则与 `mcp.port` 同源（`port_file::data_dir`）。
pub fn log_dir() -> Result<PathBuf, AppError> {
    let dir = log_dir_at(&port_file::data_dir()?);
    std::fs::create_dir_all(&dir).map_err(AppError::Io)?;
    Ok(dir)
}

/// 把一份导出写进 `dir`，返回落盘结果；没有可导出的内容时返回 `None`，此时**不创建**
/// 目录也不写文件——落一个 0 字节的文件对反馈问题毫无用处。
pub fn export_to_disk_in(dir: &Path, scope: &str) -> Result<Option<LogExportFile>, AppError> {
    let doc = export(scope)?;
    if doc.content.trim().is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(dir).map_err(AppError::Io)?;
    let path = dir.join(&doc.file_name);
    std::fs::write(&path, &doc.content).map_err(AppError::Io)?;
    Ok(Some(LogExportFile {
        file_name: doc.file_name,
        path: path.to_string_lossy().into_owned(),
    }))
}

/// 把一份导出写进 [`log_dir`]。
pub fn export_to_disk(scope: &str) -> Result<Option<LogExportFile>, AppError> {
    export_to_disk_in(&log_dir()?, scope)
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
    // `source` 进的是记录的 target，和正文一样来自渲染进程：不截它，一条上报就能让
    // target 涨到 MB 级，2000 条的缓冲直接吃满几百 GiB 的量级。
    let source = clamp_chars(source, CLIENT_FIELD_LIMIT_CHARS);
    // 先脱敏再截断：`[redacted]` 比它替换掉的短取值更长，反过来的顺序会把标记切坏。
    let mut body = clamp_chars(&redact_credentials(message), CLIENT_FIELD_LIMIT_CHARS);
    if let Some(detail) = detail.map(str::trim).filter(|d| !d.is_empty()) {
        body.push('\n');
        body.push_str(&clamp_chars(
            &redact_credentials(detail),
            CLIENT_FIELD_LIMIT_CHARS,
        ));
    }
    logbuf::record(level, &format!("webview:{source}"), body);
    Ok(())
}

/// 抹掉的取值换成的占位符。
const REDACTED: &str = "[redacted]";

/// 需要抹掉取值的 `Authorization` scheme，小写比较。
const AUTH_SCHEMES: [&str; 2] = ["bearer", "basic"];

/// 需要抹掉取值的 JSON 键名，小写比较。
const CREDENTIAL_KEYS: [&str; 2] = ["token", "authtoken"];

/// 把日志正文里的凭据取值换成 [`REDACTED`]，其余内容原样保留。
///
/// 日志会被导出成文件交给别人看，故 `Authorization: Bearer <token>`、`Bearer <token>` 与
/// `"authToken": "<token>"` 不能原样带走。这里不引 `regex`：要认的形态只有下面几类，一遍
/// 线性扫描就够，为一个脱敏多加一个依赖不划算。
fn redact_credentials(text: &str) -> String {
    redact_json_fields(&redact_scheme_tokens(text))
}

/// `text[at..]` 处是否以 `needle` 开头（ASCII 大小写不敏感）。`needle` 全是 ASCII，故按
/// 字节比较，切点永远落在边界上。
fn matches_ascii_at(text: &str, at: usize, needle: &str) -> bool {
    text.get(at..at + needle.len())
        .is_some_and(|w| w.eq_ignore_ascii_case(needle))
}

/// scheme 后面那串值得算进凭据里吗。空白、引号、分隔符与 JSON 收尾符都算边界：取值本身
/// 是 base64url / base64，不含这些字符。
fn is_token_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '"' | '\'' | ',' | ';' | '}' | ']' | '>')
}

/// 在 `at` 处认一个 `Bearer <x>` / `Basic <x>`，返回取值的字节区间。
///
/// 前一个字符必须是边界，否则 `BearerToken` 这类普通标识符会被误伤。
fn scheme_token_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let before_is_boundary = text[..at]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && c != '_');
    if !before_is_boundary {
        return None;
    }
    for scheme in AUTH_SCHEMES {
        if !matches_ascii_at(text, at, scheme) {
            continue;
        }
        let after = at + scheme.len();
        // scheme 与取值之间至少一个空白；紧贴着的（`BasicAuth`）不是这里的形态。
        let mut start = after;
        let mut saw_space = false;
        while let Some(c) = text[start..].chars().next() {
            if !c.is_whitespace() {
                break;
            }
            saw_space = true;
            start += c.len_utf8();
        }
        if !saw_space {
            return None;
        }
        let mut end = start;
        for c in text[start..].chars() {
            if !is_token_char(c) {
                break;
            }
            end += c.len_utf8();
        }
        return (end > start).then_some((start, end));
    }
    None
}

fn redact_scheme_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        match scheme_token_at(text, i) {
            Some((value_start, value_end)) => {
                out.push_str(&text[copied..value_start]);
                out.push_str(REDACTED);
                copied = value_end;
                i = value_end;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// 在 `at` 处认一个 `"token": "<x>"` / `"authToken": "<x>"`，返回取值（引号内）的字节区间。
fn json_credential_value_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let rest = text.get(at..)?;
    if !rest.starts_with('"') {
        return None;
    }
    let key_end = at + 1 + rest[1..].find('"')?;
    let key = &text[at + 1..key_end];
    if !CREDENTIAL_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) {
        return None;
    }
    let mut i = key_end + 1;
    i += text[i..].len() - text[i..].trim_start_matches([' ', '\t']).len();
    if text.get(i..)?.chars().next()? != ':' {
        return None;
    }
    i += 1;
    i += text[i..].len() - text[i..].trim_start_matches([' ', '\t']).len();
    if text.get(i..)?.chars().next()? != '"' {
        return None;
    }
    let value_start = i + 1;
    let mut j = value_start;
    while let Some(c) = text[j..].chars().next() {
        if c == '\\' {
            // 跳过转义的两个字符，否则 `\"` 会被当成取值的结尾。
            j += c.len_utf8();
            j += text.get(j..)?.chars().next()?.len_utf8();
            continue;
        }
        if c == '"' {
            return Some((value_start, j));
        }
        if c == '\n' {
            return None;
        }
        j += c.len_utf8();
    }
    None
}

fn redact_json_fields(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        match json_credential_value_at(text, i) {
            Some((value_start, value_end)) => {
                out.push_str(&text[copied..value_start]);
                out.push_str(REDACTED);
                copied = value_end;
                i = value_end;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// 按字符（不是字节）截断，故多字节字符不会被切坏。
fn clamp_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

// 测试——范围取值、文件名与前端上报的正文形状。

#[cfg(test)]
#[path = "log_api_test.rs"]
mod tests;
