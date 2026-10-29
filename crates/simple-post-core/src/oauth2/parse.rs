//! token 端点应答与用户粘贴原文的解析。

use crate::error::AppError;

use super::form::form_decode;
use super::MAX_ERROR_DETAIL;

/// token 端点的**非 2xx** 应答 → 给用户看的那句话。
///
/// 成功分支不在这儿：走 `oauth2` crate 自己的 `BasicTokenResponse`
/// （见 [`super::token_endpoint`]），而少数 IdP 回的
/// `application/x-www-form-urlencoded` 由 [`normalize_success_body`] 先折成 JSON。
/// 非 2xx 必须在 crate 看到应答**之前**变成 [`AppError`]——crate 只认 200，
/// 且它的错误分支是英文的，也不裁剪长度。
pub(super) fn error_response(status: u16, body: &str, content_type: &str) -> AppError {
    let pairs = read_response_pairs(body, content_type);
    let code = lookup(&pairs, "error").unwrap_or_else(|| format!("http_{status}"));
    let detail = lookup(&pairs, "error_description");
    AppError::Validation(match detail {
        Some(d) if !d.trim().is_empty() => format!("token 端点返回 {status} {code}: {}", clip(&d)),
        _ => format!("token 端点返回 {status} {code}"),
    })
}

/// 成功应答的 body → crate 能解析的 JSON。
///
/// crate 的 `StandardTokenResponse` 比现实严格两处，这两处都得在这里补平，否则那批
/// IdP 会从「能取到 token」变成一句解析错误：
///
/// - `token_type` 在 crate 里是**必填**，但 §5.1 之外仍有 IdP 不回；缺时补
///   `Bearer`（与本项目默认的 `token_type` 一致）。
/// - `expires_in` 在 crate 里是 `Option<u64>`，即必须是 JSON **数字**；而
///   form-urlencoded 的应答给不了类型，统一转成字符串后得再按数字转回来。
///
/// 另有一处：少数 IdP 回 `application/x-www-form-urlencoded`，而 crate 只解 JSON，
/// 原样透传会让那批 IdP 直接解析失败，故折一次。
///
/// 刻意**不做**递归扁平化：§5.1 的响应是扁平的，再拆一层只会多出一批取用不到的 `a.b` 键。
pub(super) fn normalize_success_body(body: &str, content_type: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let mut map = match parsed {
        Some(serde_json::Value::Object(m)) => m,
        // 不是 JSON 对象（多半是 form-urlencoded）：先折成键值对。
        _ => read_response_pairs(body, content_type)
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect(),
    };

    // crate 的 `token_type` 必填；缺或空都补 `Bearer`。
    let missing_type = match map.get("token_type") {
        None => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        Some(_) => false,
    };
    if missing_type {
        map.insert(
            "token_type".to_string(),
            serde_json::Value::String("Bearer".into()),
        );
    }

    // `expires_in` 必须是数字。form 路径把它转成了字符串，这里转回来；
    // 已经是数字或无法解析时原样留着，让 crate 自己报缺字段。
    if let Some(serde_json::Value::String(s)) = map.get("expires_in") {
        if let Ok(n) = s.trim().parse::<u64>() {
            map.insert(
                "expires_in".to_string(),
                serde_json::Value::Number(n.into()),
            );
        }
    }

    serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_else(|_| body.to_string())
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

/// 按 content-type 选解析器。
///
/// content-type 明写 form-urlencoded 才走表单分支；其余一律先试 JSON —— IdP 漏发
/// content-type、或把它写成 `text/html` 的都不少，而 body 其实是 JSON。
fn read_response_pairs(body: &str, content_type: &str) -> Vec<(String, String)> {
    if content_type.contains("x-www-form-urlencoded") {
        return form_pairs(body.trim());
    }
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(body) {
        return map
            .into_iter()
            .map(|(k, v)| (k, json_scalar_to_string(&v)))
            .filter(|(_, v)| !v.is_empty())
            .collect();
    }
    form_pairs(body.trim())
}

/// 只取标量。对象 / 数组没有「字符串值」可言，再拆一层只会造出取用不到的键。
fn json_scalar_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
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

/// 把三段真实管线收成一个可直调的入口，**只给单测用**。
///
/// 线上那三段分在两个地方：`http_client::exchange` 先把非 2xx 转成错误、把 body
/// 归一成 JSON，crate 再把那份 JSON 解成 `BasicTokenResponse`，最后
/// `token_endpoint::to_token` 映射成本项目的 token。单测没有网络可走，于是按同样
/// 的顺序调同一批函数——这里不重写任何逻辑，只是不让单测为了断网而复制一份。
#[cfg(test)]
pub(super) fn parse_token_response(
    status: u16,
    body: &str,
    content_type: &str,
) -> Result<super::OAuth2Token, crate::error::AppError> {
    if !(200..300).contains(&status) {
        return Err(error_response(status, body, content_type));
    }
    let normalized = normalize_success_body(body, content_type);
    let resp: oauth2::basic::BasicTokenResponse = serde_json::from_str(&normalized)
        .map_err(|_| crate::error::AppError::Validation("token 响应里没有 access_token".into()))?;
    Ok(super::token_endpoint::to_token(resp))
}
