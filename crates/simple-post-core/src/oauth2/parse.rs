//! token 端点应答与用户粘贴原文的解析。

use crate::error::AppError;

use super::form::form_decode;
use super::{OAuth2Token, MAX_ERROR_DETAIL};

/// token 响应 → [`OAuth2Token`]。RFC 6749 §5.1 定义 JSON；少数 IdP 仍回
/// `application/x-www-form-urlencoded`，故两种都收。
pub(super) fn parse_token_response(
    status: u16,
    body: &str,
    content_type: &str,
) -> Result<OAuth2Token, AppError> {
    let pairs = read_response_pairs(body, content_type);

    if !(200..300).contains(&status) {
        let code = lookup(&pairs, "error").unwrap_or_else(|| format!("http_{status}"));
        let detail = lookup(&pairs, "error_description");
        return Err(AppError::Validation(match detail {
            Some(d) if !d.trim().is_empty() => {
                format!("token 端点返回 {status} {code}: {}", clip(&d))
            }
            _ => format!("token 端点返回 {status} {code}"),
        }));
    }

    let access_token = lookup(&pairs, "access_token").unwrap_or_default();
    if access_token.is_empty() {
        return Err(AppError::Validation("token 响应里没有 access_token".into()));
    }
    Ok(OAuth2Token {
        access_token,
        // 刷新响应按 §6 可能不重复 refresh_token，调用方负责沿用旧值。
        refresh_token: lookup(&pairs, "refresh_token").filter(|v| !v.is_empty()),
        token_type: lookup(&pairs, "token_type")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "Bearer".into()),
        scope: lookup(&pairs, "scope").filter(|v| !v.is_empty()),
        expires_at: lookup(&pairs, "expires_in")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|s| *s > 0)
            .map(|s| super::store::unix_secs() + s),
        obtained_at: super::store::unix_secs(),
    })
}

/// 用户粘贴的原文 → token 各字段。支持整条回调 URL（`#access_token=…` 或
/// `?access_token=…`）与裸 token 串。
pub(super) struct PastedToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: Option<i64>,
}

pub(super) fn parse_pasted(pasted: &str) -> Result<PastedToken, AppError> {
    let raw = pasted.trim();
    if raw.is_empty() {
        return Err(AppError::Validation("粘贴的内容是空的".into()));
    }

    // 回调 URL：query 与 fragment 都要看——IdP 走 implicit 时把 token 放 fragment
    // （RFC 6749 §4.2.2），走自定义回调时有人放 query。
    for sep in ['?', '#'] {
        let Some(pos) = raw.find(sep) else { continue };
        let pairs = form_pairs(&raw[pos + 1..]);
        if let Some(at) = lookup(&pairs, "access_token").filter(|v| !v.is_empty()) {
            return Ok(PastedToken {
                access_token: at,
                refresh_token: lookup(&pairs, "refresh_token").filter(|v| !v.is_empty()),
                token_type: lookup(&pairs, "token_type")
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| "Bearer".into()),
                expires_in: lookup(&pairs, "expires_in").and_then(|v| v.trim().parse::<i64>().ok()),
            });
        }
    }

    // 裸 token（dashboard 复制、CLI 输出）。
    Ok(PastedToken {
        access_token: raw.to_string(),
        refresh_token: None,
        token_type: "Bearer".into(),
        expires_in: None,
    })
}

/// 从「授权码」输入里取出真正的 `code`。接受三种写法：整条回调 URL、裸 code、
/// `code=xxx`。URL 的 query 与 fragment 都看——回环回调常把 code 放 fragment。
///
/// **像回调 URL 却取不到 code 时返回空串**，好让 `validate` 报「需要授权码」。
/// 把整条 URL 当 code 送出去只会在 IdP 那边换来一个更费解的 400。
pub(super) fn extract_code(raw: &str) -> String {
    let raw = raw.trim();
    let mut looks_like_callback = false;
    for sep in ['?', '#'] {
        let Some(pos) = raw.find(sep) else { continue };
        looks_like_callback = true;
        if let Some(code) = lookup(&form_pairs(&raw[pos + 1..]), "code").filter(|v| !v.is_empty()) {
            return code;
        }
    }
    if looks_like_callback {
        return String::new();
    }
    if let Some(rest) = raw.strip_prefix("code=") {
        return form_decode(rest);
    }
    raw.to_string()
}

/// `a=1&b=2` → 键值对。`&` / `=` 两端都可缺（`?flag` / `=v`）。
fn form_pairs(qs: &str) -> Vec<(String, String)> {
    qs.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (form_decode(k), form_decode(v)),
            None => (form_decode(pair), String::new()),
        })
        .collect()
}

fn lookup(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.to_string())
}

/// 按 content-type 选解析器；没有该头时先试 JSON——IdP 漏发 content-type 的不少。
fn read_response_pairs(body: &str, content_type: &str) -> Vec<(String, String)> {
    if content_type.contains("x-www-form-urlencoded") {
        return form_pairs(body.trim());
    }
    if content_type.contains("json") || content_type.is_empty() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            return flatten_json(&v);
        }
    }
    // 兜底：content-type 写着别的东西，但 body 其实是 JSON（太常见了）。
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        return flatten_json(&v);
    }
    form_pairs(body.trim())
}

/// OAuth2 的响应体是**扁平**的（§5.1），但 IdP 常在同一个 JSON 里附带别的信息，
/// 故递归摊平成 `a.b` 形式，取顶层键的行为不变。
fn flatten_json(v: &serde_json::Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    flatten_into(v, "", &mut out);
    out
}

fn flatten_into(v: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_into(val, &key, out);
            }
        }
        serde_json::Value::String(s) => out.push((prefix.to_string(), s.clone())),
        serde_json::Value::Number(n) => out.push((prefix.to_string(), n.to_string())),
        serde_json::Value::Bool(b) => out.push((prefix.to_string(), b.to_string())),
        _ => {}
    }
}

pub(super) fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_ERROR_DETAIL {
        return s;
    }
    let cut: String = s.chars().take(MAX_ERROR_DETAIL).collect();
    format!("{cut}…")
}
