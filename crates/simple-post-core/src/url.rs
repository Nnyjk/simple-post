//! URL 的百分号编码 —— 与渲染进程的 `encodeURIComponent` 逐字节一致。
//!
//! 两侧拼查询串都用它：渲染进程用 JS 内置的 `encodeURIComponent`，后端用本模块。
//! 两者保留的字符集必须相同，否则同一个参数在界面预览与 MCP 侧会得到两个 URL
//! ——而「预览描述的就是发出去的那个请求」是本项目的一条不变式。

/// 百分号编码。保留 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`，其余字节一律 `%XX`（大写十六进制）。
///
/// 字符集取 JS 的 `encodeURIComponent` 而不是 RFC 3986 的 unreserved 集：多出来的
/// `! * ' ( )` 在查询串里是合法的子分隔符。空格是 `%20`（不是 form 编码的 `+`）：`+` 在
/// 严格按 RFC 3986 解析查询串的服务端会被读成加号而非空格。
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            other => {
                out.push('%');
                out.push(hex_upper(other >> 4));
                out.push(hex_upper(other & 0x0F));
            }
        }
    }
    out
}

use crate::domain::RewriteRule;
use crate::error::AppError;

/// 前导 `scheme://` 的字节长度，没有则 `None`。
///
/// RFC 3986 的 scheme 是 `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`，后面跟 `:` 与 `//`。
/// 渲染进程侧用 `/^[a-z][a-z0-9+.-]*:\/\//i` 认同一件事，两边必须一致。
pub fn scheme_prefix_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    if i + 2 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b'/' && bytes[i + 2] == b'/' {
        Some(i + 3)
    } else {
        None
    }
}

/// 规范化一条改写规则的起点，用于判重。
///
/// 去首尾空白，再去掉末尾所有 `/`；去掉后为空（如 `"/"`、`"///"`）时返回 `"/"`。
/// 故 `"/api/air"` 与 `"/api/air/"` 是同一个起点，`"/"` 与 `"///"` 也是。
pub fn normalize_rewrite_from(from: &str) -> String {
    let trimmed = from.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let stripped = trimmed.trim_end_matches('/');
    if stripped.is_empty() {
        "/".to_string()
    } else {
        stripped.to_string()
    }
}

/// 单条规则对 URL 的改写结果；不命中返回 `None`。
///
/// 规则只作用在路径上：主机与查询串一概不动。
///
/// - `from` 去掉首尾空白之后为空 → `None`（没有起点就没有规则）。
/// - `to` 是绝对地址（带 `scheme://` 或以 `//` 开头）→ `None`。改写只准改路径，
///   不准把请求挪到另一个主机上去——那会把凭据发到别处。
/// - `from` 去掉尾部所有 `/`；起点就是 `/` 时它指的是**路径根**，整条路径换到 `to` 底下。
/// - 路径以 `from` 开头，**且**其后紧跟的那一位是 `/`（或路径恰好等于 `from`）→ 换成
///   `to` 加其余部分。边界这一条是必需的：没有它，`/api/air` 会命中 `/api/airplane`。
///
/// `from` 按**原文**匹配：它不做 `{{var}}` 替换，故那里的标记永远不会被展开。`to` 会被
/// 随后的 `{{var}}` 替换扫到（它进了 URL）。
///
/// 与渲染进程的 `src/lib/url.ts::tryApplyRewrite` 对应；两者必须一致，否则预览与真正发出去的
/// 请求描述的是两个 URL。共享样本是 `fixtures/base-url-parity.json`。
pub fn try_apply_rewrite(url: &str, rule: &RewriteRule) -> Option<String> {
    let from_raw = rule.from.trim();
    if from_raw.is_empty() {
        return None;
    }
    let to = rule.to.trim();
    if is_absolute_target(to) {
        return None;
    }

    let (head, path, tail) = split_url(url);
    let from = from_raw.trim_end_matches('/');
    let new_path = if from.is_empty() {
        // 起点是路径根：整条路径换到目标底下。`to` 与路径各自带的那道斜杠都要留着，
        // 故这里是直接相接（`/gateway` + `/api/air/list`）。
        format!("{to}{path}")
    } else {
        let rest = path.strip_prefix(from)?;
        if !rest.is_empty() && !rest.starts_with('/') {
            return None;
        }
        format!("{to}{rest}")
    };
    Some(format!("{head}{new_path}{tail}"))
}

/// 按列表顺序依次尝试改写，**命中第一条后立即返回**（不串联执行后续规则）；
/// 全部未命中时原样返回。
pub fn apply_rewrites(url: &str, rules: &[RewriteRule]) -> String {
    for rule in rules {
        if let Some(next) = try_apply_rewrite(url, rule) {
            return next;
        }
    }
    url.to_string()
}

/// 校验一组改写规则：起点不得为空、目标不得是绝对地址、同一组内规范化起点不得重复。
///
/// 重复的起点意味着后面那条永远匹配不到（首条命中即停止），故在写入口直接拒绝。
/// 调用方应先 [`clean_rewrites`] 掉全空白占位行，否则占位行会被判成「起点为空」。
pub fn validate_rewrites(rules: &[RewriteRule]) -> Result<(), AppError> {
    let mut seen = std::collections::HashSet::new();
    for rule in rules {
        if rule.from.trim().is_empty() {
            return Err(AppError::Validation(
                "路径改写规则的起点不能为空".to_string(),
            ));
        }
        if is_absolute_target(rule.to.trim()) {
            return Err(AppError::Validation(
                "路径改写目标不能包含协议或主机地址".to_string(),
            ));
        }
        let key = normalize_rewrite_from(&rule.from);
        if !seen.insert(key.clone()) {
            return Err(AppError::Validation(format!(
                "模块路径改写存在重复起点「{key}」"
            )));
        }
    }
    Ok(())
}

/// 剔除全空白的占位行（`from` 与 `to` 去空白后都为空），保留其余规则的原样与顺序。
pub fn clean_rewrites(rules: &[RewriteRule]) -> Vec<RewriteRule> {
    rules
        .iter()
        .filter(|r| !(r.from.trim().is_empty() && r.to.trim().is_empty()))
        .cloned()
        .collect()
}

/// 目标是不是一个绝对地址。带 `scheme://` 或以 `//` 开头的都算。
fn is_absolute_target(to: &str) -> bool {
    scheme_prefix_len(to).is_some() || to.starts_with("//")
}

/// 把 URL 拆成三段：主机部分（含 scheme）、路径、查询串与片段。
///
/// 主机部分含 scheme 与 authority：`https://h`、`//h`、空串（相对地址）都算主机部分。
/// 路径是从主机部分之后到 `?` 或 `#` 之前的那一段。
fn split_url(url: &str) -> (&str, &str, &str) {
    let (head, rest) = match scheme_prefix_len(url) {
        Some(n) => {
            let after = &url[n..];
            let end = after.find(['/', '?', '#']).unwrap_or(after.len());
            (&url[..n + end], &after[end..])
        }
        None if url.starts_with("//") => {
            let after = &url[2..];
            let end = after.find(['/', '?', '#']).unwrap_or(after.len());
            (&url[..2 + end], &after[end..])
        }
        None => ("", url),
    };
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let (path, tail) = rest.split_at(cut);
    (head, path, tail)
}

fn hex_upper(nibble: u8) -> char {
    char::from_digit(nibble as u32, 16)
        .unwrap_or('0')
        .to_ascii_uppercase()
}

#[cfg(test)]
#[path = "url_test.rs"]
mod tests;
