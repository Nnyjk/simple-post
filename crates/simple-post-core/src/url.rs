//! URL 的百分号编码 —— 与渲染进程的 `encodeURIComponent` 逐字节一致。
//!
//! 两侧拼查询串都用它：渲染进程用 JS 内置的 `encodeURIComponent`，后端用本模块。
//! 两者保留的字符集必须相同，否则同一个参数在界面预览与 MCP 侧会得到两个 URL
//! ——而「预览描述的就是发出去的那个请求」是本项目的一条不变式。

/// 百分号编码。保留 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`，其余字节一律 `%XX`（大写十六进制）。
///
/// 字符集取 JS 的 `encodeURIComponent` 而不是 RFC 3986 的 unreserved 集：多出来的
/// `! * ' ( )` 在查询串里是合法的子分隔符，而渲染进程发出去的请求用的就是这套编码。
/// 空格是 `%20`（不是 form 编码的 `+`）：`+` 在严格按 RFC 3986 解析查询串的服务端
/// 会被读成加号而非空格。
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

fn hex_upper(nibble: u8) -> char {
    char::from_digit(nibble as u32, 16)
        .unwrap_or('0')
        .to_ascii_uppercase()
}

#[cfg(test)]
#[path = "url_test.rs"]
mod tests;
