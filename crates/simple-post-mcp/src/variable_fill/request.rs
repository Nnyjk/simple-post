//! 请求头排序、query 追加与 Basic 鉴权的 base64 —— `resolve_endpoint` 的零件。
//!
//! 请求头的顺序规则（用户头 → 自动 Content-Type → Accept → 鉴权推出的
//! `Authorization`）就固化在 `build_headers` 里。

use std::collections::HashMap;

use simple_post_core::domain::{ApiKeyLocation, AuthConfig, AuthType};

use super::template::resolve_template;
// -------------------------------------------------------------------------
// URL / header / param / body 组装
// -------------------------------------------------------------------------

/// 构建端点的完整有序请求头列表。
///
/// 顺序：用户请求头（启用且非空）→ body 模式蕴含时补的 `Content-Type`（缺失才补）
/// → 默认 `Accept: application/json` → 鉴权推出的 `Authorization`（或作为请求头的
/// apikey）。重复键按大小写不敏感判断（HTTP 请求头名本就不敏感）。
///
/// `apikey.inLocation == Query` 在这里**不**生成请求头 —— 由调用方经
/// `append_apikey_query` 追加到 URL。
pub(super) fn build_headers(
    endpoint_headers: &[simple_post_core::domain::KeyValue],
    body_mode: &simple_post_core::domain::BodyMode,
    auth: &AuthConfig,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out: Vec<(String, String)> = Vec::new();

    let record_missing = |out: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            out.push(var);
        }
    };

    for h in endpoint_headers {
        if !h.enabled || h.key.is_empty() {
            continue;
        }
        let r = resolve_template(&h.value, vars);
        for m in r.missing {
            record_missing(missing, &mut seen, m);
        }
        out.push((h.key.clone(), r.resolved));
    }

    if let Some(ct) = auto_content_type(body_mode) {
        if !out
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            out.push(("Content-Type".to_string(), ct.to_string()));
        }
    }
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) {
        out.push(("Accept".to_string(), "application/json".to_string()));
    }

    match (&auth.kind, auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) if !b.is_empty() => {
            if !out
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            {
                out.push(("Authorization".to_string(), format!("Bearer {b}")));
            }
        }
        (AuthType::Basic, _) => {
            if let Some(b) = &auth.basic {
                if !out
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                {
                    let token = b64_basic(&b.username, &b.password);
                    out.push(("Authorization".to_string(), format!("Basic {token}")));
                }
            }
        }
        (AuthType::Apikey, _) => {
            if let Some(a) = &auth.apikey {
                if a.in_location == ApiKeyLocation::Header
                    && !out.iter().any(|(k, _)| k.eq_ignore_ascii_case(&a.key))
                {
                    out.push((a.key.clone(), a.value.clone()));
                }
            }
        }
        _ => {}
    }

    out
}

fn auto_content_type(mode: &simple_post_core::domain::BodyMode) -> Option<&'static str> {
    use simple_post_core::domain::BodyMode;
    match mode {
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Raw => Some("text/plain"),
        BodyMode::None => None,
    }
}

pub(super) fn append_apikey_query(url: String, auth: &AuthConfig) -> String {
    if let AuthConfig {
        kind: AuthType::Apikey,
        apikey: Some(a),
        ..
    } = auth
    {
        if a.in_location == ApiKeyLocation::Query && !a.key.is_empty() {
            let glue = if url.contains('?') { '&' } else { '?' };
            return format!("{url}{glue}{}={}", url_encode(&a.key), url_encode(&a.value));
        }
    }
    url
}

pub(super) fn append_query_string(mut url: String, params: &[(String, String)]) -> String {
    let mut first = !url.contains('?');
    for (k, v) in params {
        if k.is_empty() || v.is_empty() {
            continue;
        }
        let glue = if first { '?' } else { '&' };
        url.push(glue);
        url.push_str(&url_encode(k));
        url.push('=');
        url.push_str(&url_encode(v));
        first = false;
    }
    url
}

fn url_encode(s: &str) -> String {
    // 按 `application/x-www-form-urlencoded` 编码（与 TS 侧的 `encodeURIComponent`
    // 一致）：空格 → `+`，unreserved 集合之外一律 → 大写十六进制 `%XX`。
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 为 HTTP Basic 鉴权把 `user:pass` 做 base64。先过 UTF-8 字节，所以含非 ASCII 的
/// 凭据（中文用户名、emoji 密码）也编码正确 —— 与浏览器、以及 Express 等多数服务端
/// 的约定一致（把请求头当裸字节解码）。
fn b64_basic(user: &str, pass: &str) -> String {
    let combined = format!("{user}:{pass}");
    base64_encode(combined.as_bytes())
}

/// 极简 base64 编码器，标准字母表（`A-Z a-z 0-9 + /`）、`=` 补齐。只为
/// `Authorization: Basic` 而存在 —— 为此拉一个 `base64` crate 不如手写这 30 行。
fn base64_encode(input: &[u8]) -> String {
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
