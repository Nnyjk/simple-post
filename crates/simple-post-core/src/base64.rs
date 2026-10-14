//! 极简 base64 编码器。
//!
//! 只有两处用它：OAuth2 token 端点的 `Authorization: Basic`，以及 `simple-post-mcp`
//! 里拼 `Authorization: Basic` 的 HTTP Basic 鉴权。两处都要的是同一件事，故实现在
//! core 的这一个函数里，由 mcp crate 复用——两份实现迟早会对不上（补位、换行、非
//! ASCII 的处理都是可以悄悄写错的地方）。
//!
//! 只做编码，不做解码：本项目没有任何地方要读回 base64。

/// 标准字母表（`A-Z a-z 0-9 + /`）与 `=` 补齐，即 RFC 4648 §4。
pub fn encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
#[path = "base64_test.rs"]
mod tests;
