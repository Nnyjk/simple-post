//! 请求头排序、query 追加与 Basic 鉴权的 base64 —— `resolve_endpoint` 的零件。
//!
//! 请求头的顺序规则（用户头 → 自动 Content-Type → Accept → 鉴权推出的
//! `Authorization`）就固化在 `build_headers` 里。

use std::collections::HashMap;

use base64::Engine as _;

use simple_post_core::domain::{ApiKeyLocation, AuthConfig, AuthType};
use simple_post_core::url::percent_encode;

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
///
/// `binary_content_type` 是 `binary` 模式下文件自己的 MIME 类型：整个请求体是一个
/// 文件时，用文件那个类型才描述得了它（一张 PNG 不该顶着 `application/octet-stream`
/// 发出去）。它只在 `body_mode` 是 `Binary` 时被读，其余模式传 `None`。
pub(super) fn build_headers(
    endpoint_headers: &[simple_post_core::domain::KeyValue],
    body_mode: &simple_post_core::domain::BodyMode,
    auth: &AuthConfig,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
    binary_content_type: Option<&str>,
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

    if let Some(ct) = auto_content_type(body_mode, binary_content_type) {
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

fn auto_content_type<'a>(
    mode: &simple_post_core::domain::BodyMode,
    binary_content_type: Option<&'a str>,
) -> Option<&'a str> {
    use simple_post_core::domain::BodyMode;
    match mode {
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Xml => Some("application/xml"),
        BodyMode::Raw => Some("text/plain"),
        // `binary` 模式整个请求体就是一个文件，故 Content-Type 取那个文件的类型；
        // 读不出文件类型时用二进制流类型——任何服务端都能收下它。
        BodyMode::Binary => Some(binary_content_type.unwrap_or("application/octet-stream")),
        BodyMode::None => None,
        // multipart 的 Content-Type 必须带 boundary，而 boundary 每次请求现生成，
        // 纯函数给不出来。返回不带 boundary 的值等于发出一个服务端解析不了的请求，
        // 故这条留空，由发送方（`mcp::http_ops`）在 execute 之前补上。
        BodyMode::Multipart => None,
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
            return format!(
                "{url}{glue}{}={}",
                percent_encode(&a.key),
                percent_encode(&a.value)
            );
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
        url.push_str(&percent_encode(k));
        url.push('=');
        url.push_str(&percent_encode(v));
        first = false;
    }
    url
}

/// 为 HTTP Basic 鉴权把 `user:pass` 做 base64。先过 UTF-8 字节，所以含非 ASCII 的
/// 凭据（中文用户名、emoji 密码）也编码正确 —— 与浏览器、以及 Express 等多数服务端
/// 的约定一致（把请求头当裸字节解码）。
fn b64_basic(user: &str, pass: &str) -> String {
    let combined = format!("{user}:{pass}");
    // 标准字母表而非 URL-safe：`Authorization: Basic` 的 RFC 7617 没有 -_ 那两个。
    base64::engine::general_purpose::STANDARD.encode(combined.as_bytes())
}
