//! `application/x-www-form-urlencoded` 的编解码原语。
//!
//! 编码只服务 query 档的 token 注入（[`super::inject`]`::auth_query_value`）；
//! token 请求的 form 编码由 `oauth2` crate 做，故这里没有整表序列化。

/// `application/x-www-form-urlencoded` 编码：未保留字符一律 `%XX`，空格是 `+`。
/// 与 `url::form_urlencoded` 的输出逐字节一致——不引 `url` crate 只为这 20 行。
pub(super) fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(hex_upper(other >> 4));
                out.push(hex_upper(other & 0x0F));
            }
        }
    }
    out
}

pub(super) fn form_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_upper(nibble: u8) -> char {
    char::from_digit(nibble as u32, 16)
        .unwrap_or('0')
        .to_ascii_uppercase()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
