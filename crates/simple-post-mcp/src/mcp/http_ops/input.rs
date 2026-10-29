//! 工具入参 → 请求形状：键值对压平、鉴权与 body 模式解析、设置覆盖、verb 还原。
//!
//! `flatten_string_map` 给 `response_parse` 也用，所以是 `pub(crate)`，并由门面
//! 重新导出在 `crate::mcp::http_ops::flatten_string_map`。

use serde_json::Value;

use super::RunRequestArgs;
use simple_post_core::domain::{AuthConfig, BodyMode, KeyValue, TlsSettings};
use simple_post_core::error::AppError;
// =========================================================================
// 共享辅助函数
// =========================================================================

/// 把 JSON 对象（`{"Foo": "bar"}`）或键值对数组（`[["Foo","bar"]]`）压平成
/// `Vec<(String, String)>`。
///
/// `label` 用来给形状错误的提示加前缀。这里返回 `Result` 而不是静默丢弃：
/// 输入来自 AI 客户端，静默丢形状等于凭空造一个「成功但少了几个头」的结果。
pub(crate) fn flatten_string_map(
    v: Option<&Value>,
    label: &str,
) -> Result<Vec<(String, String)>, AppError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if let Value::Object(map) = v {
        for (k, val) in map {
            let s = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => continue,
                other => other.to_string(),
            };
            out.push((k.clone(), s));
        }
        return Ok(out);
    }
    if let Value::Array(arr) = v {
        for item in arr {
            if let Value::Array(pair) = item {
                if pair.len() >= 2 {
                    // 键与值都走标量转换：`Value::String("X").to_string()` 会带上引号，
                    // header 名就成了 `"X"` —— 服务端看到的是一个不存在的头。
                    out.push((scalar_to_string(&pair[0]), scalar_to_string(&pair[1])));
                }
            } else {
                return Err(AppError::Validation(format!(
                    "{label}: array items must be [key, value] pairs"
                )));
            }
        }
        return Ok(out);
    }
    Err(AppError::Validation(format!(
        "{label}: must be object or array of pairs"
    )))
}

/// 把 JSON 压成请求用的键值对列表（`KeyValue`）。
///
/// 接受三种形状，因为 AI 客户端写哪种都有：
///   - `{"Foo": "bar"}` —— 对象映射
///   - `[["Foo","bar"]]` —— 键值对数组
///   - `[{"key": "Foo", "value": "bar", "enabled": false}]` —— 端点那套形状
///
/// `id` 由这里按序号生成：它只用于让同一批键值对在编辑器里各自可寻，解析请求时
/// 不用它。
pub(crate) fn flatten_key_values(
    v: Option<&Value>,
    label: &str,
) -> Result<Vec<KeyValue>, AppError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut push = |key: String, value: String, enabled: Option<bool>| {
        let id = format!("{label}_{}", out.len());
        out.push(KeyValue {
            id,
            key,
            value,
            enabled: enabled.unwrap_or(true),
            description: None,
        });
    };
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let s = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => continue,
                    other => other.to_string(),
                };
                push(k.clone(), s, None);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                match item {
                    Value::Array(pair) if pair.len() >= 2 => {
                        let key = scalar_to_string(&pair[0]);
                        let value = scalar_to_string(&pair[1]);
                        push(key, value, None);
                    }
                    Value::Object(obj) => {
                        let key = obj.get("key").map(scalar_to_string).unwrap_or_default();
                        let value = obj.get("value").map(scalar_to_string).unwrap_or_default();
                        let enabled = obj.get("enabled").and_then(Value::as_bool);
                        push(key, value, enabled);
                    }
                    _ => {
                        return Err(AppError::Validation(format!(
                            "{label}: array items must be [key, value] pairs or {{key, value}} objects"
                        )));
                    }
                }
            }
        }
        _ => {
            return Err(AppError::Validation(format!(
                "{label}: must be an object, an array of pairs, or an array of {{key, value}} objects"
            )));
        }
    }
    Ok(out)
}

/// JSON 标量 → 字符串。对象与数组不转（它们不是请求里能出现的值）。
fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// 鉴权载荷 → [`AuthConfig`]。形状与端点存的那份一致
/// （`{"kind": "bearer", "bearer": "…"}` / `{"kind": "basic", "basic": {…}}` /
/// `{"kind": "apikey", "apikey": {…}}` / `{"kind": "oauth2", "oauth2": {…}}`）。
///
/// 载荷缺席 = 不用鉴权。形状不对就报错：静默当成「不用鉴权」会发出一个缺了凭据的
/// 请求，而调用方以为它带上了。
pub(super) fn parse_auth(v: Option<&Value>) -> Result<AuthConfig, AppError> {
    let Some(v) = v else {
        return Ok(AuthConfig::default());
    };
    if v.is_null() {
        return Ok(AuthConfig::default());
    }
    serde_json::from_value::<AuthConfig>(v.clone())
        .map_err(|e| AppError::Validation(format!("auth: {e}")))
}

/// body 模式名 → [`BodyMode`]。缺席时：有 body 就算 `raw`，没有就算 `none`。
pub(super) fn parse_body_mode(v: Option<&str>, has_body: bool) -> Result<BodyMode, AppError> {
    let Some(raw) = v else {
        return Ok(if has_body {
            BodyMode::Raw
        } else {
            BodyMode::None
        });
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(BodyMode::None),
        "json" => Ok(BodyMode::Json),
        "form" => Ok(BodyMode::Form),
        "raw" | "text" => Ok(BodyMode::Raw),
        "multipart" => Ok(BodyMode::Multipart),
        "xml" => Ok(BodyMode::Xml),
        "binary" => Ok(BodyMode::Binary),
        other => Err(AppError::Validation(format!(
            "body_mode: unknown mode {other:?} (expected none|json|form|multipart|xml|raw|binary)"
        ))),
    }
}

pub(super) fn extract_settings(
    so: &Option<Value>,
    args: &RunRequestArgs,
) -> (Option<u64>, Option<bool>, Option<u64>) {
    let mut t = args.timeout_ms;
    let mut r = args.follow_redirects;
    let mut m = args.max_response_size_kb;
    if let Some(v) = so {
        if let Some(n) = v.get("timeout_ms").and_then(Value::as_u64) {
            t = Some(n);
        }
        if let Some(b) = v.get("follow_redirects").and_then(Value::as_bool) {
            r = Some(b);
        }
        if let Some(n) = v.get("max_response_size_kb").and_then(Value::as_u64) {
            m = Some(n);
        }
    }
    (t, r, m)
}

pub(super) fn extract_endpoint_settings(
    so: &Option<Value>,
) -> (Option<u64>, Option<bool>, Option<u64>) {
    let mut t = None;
    let mut r = None;
    let mut m = None;
    if let Some(v) = so {
        t = v.get("timeout_ms").and_then(Value::as_u64);
        r = v.get("follow_redirects").and_then(Value::as_bool);
        m = v.get("max_response_size_kb").and_then(Value::as_u64);
    }
    (t, r, m)
}

// =========================================================================
// HTTPS 出站设置
// =========================================================================

/// `settings_override` 里的 `tls` 载荷 → 这次调用生效的 HTTPS 设置。
///
/// **这里的键名是 snake_case，而 `TlsSettings` 自己的 serde 键名是 camelCase**
/// （`skipVerify` / `caCertificatePath` / `clientCertificatePath` /
/// `clientKeyPath`）。`settings_override` 里既有的三个键（`timeout_ms` /
/// `follow_redirects` / `max_response_size_kb`）都是 snake_case，故 `tls` 内部
/// 也用 snake_case，与它们保持一致。
///
/// **不能把这份 JSON 直接反序列化成 `TlsSettings`**：`TlsSettings` 的每个字段都带
/// 默认值，键名对不上时 serde 不报错，而是把每一项都填成默认值 —— 于是
/// 「跳过校验」与「挂上客户端证书」会静默地全部失效，而调用方看到的是一次
/// 成功的请求。故这里逐个键读。
///
/// 键缺席或值是 `null` → 该项取默认值（`skip_verify` 为 `false`，路径为
/// `None`）。值的类型不对 → 报错并点名那个键：路径写错会让请求在**没有**客户端
/// 证书的情况下发出去，而调用方不会知道自己传的路径没被收下。
pub(super) fn parse_tls_settings(v: Option<&Value>) -> Result<Option<TlsSettings>, AppError> {
    let Some(v) = v else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    if !v.is_object() {
        return Err(AppError::Validation(format!(
            "settings_override.tls must be an object, got {v}"
        )));
    }
    Ok(Some(TlsSettings {
        skip_verify: tls_bool(v, "skip_verify")?.unwrap_or(false),
        ca_certificate_path: tls_path(v, "ca_certificate_path")?,
        client_certificate_path: tls_path(v, "client_certificate_path")?,
        client_key_path: tls_path(v, "client_key_path")?,
    }))
}

/// 合成这次请求生效的 HTTPS 设置。
///
/// 优先级从高到低：
///
/// 1. 本次调用的临时覆盖（`settings_override.tls`）；
/// 2. 端点自带的那份（`EndpointDetail::tls`）；
/// 3. 全局默认（`RequestSettings::tls`，由 `resolve_settings` 从 `preferences` 读）。
///
/// 前两档之间用 `Option::or` 合成：端点给了设置就**整体**取代全局那份，不做逐字段
/// 合并 —— 合并规则只有 `simple_post_core::tls::resolve` 那一处，理由写在那个模块的
/// 文档注释里。
///
/// 两档都缺席时返回 `None`，`send_with_scripts` 据此取全局默认。
pub(super) fn resolve_tls_override(
    settings_override: Option<&Value>,
    endpoint_tls: Option<TlsSettings>,
) -> Result<Option<TlsSettings>, AppError> {
    let override_tls = parse_tls_settings(settings_override.and_then(|v| v.get("tls")))?;
    Ok(override_tls.or(endpoint_tls))
}

/// 读一个布尔字段。键缺席或值是 `null` → `None`（调用方取默认值）；值不是布尔 →
/// 报错并点名那个键。
fn tls_bool(v: &Value, field: &str) -> Result<Option<bool>, AppError> {
    match v.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(AppError::Validation(format!(
            "settings_override.tls.{field} must be a boolean, got {other}"
        ))),
    }
}

/// 读一个路径字段。键缺席或值是 `null` → `None`（没有路径）；值不是字符串 → 报错
/// 并点名那个键。
fn tls_path(v: &Value, field: &str) -> Result<Option<String>, AppError> {
    match v.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(AppError::Validation(format!(
            "settings_override.tls.{field} must be a string path, got {other}"
        ))),
    }
}

/// `HttpMethod` 还原成线上字符串。
pub(super) fn format_method(m: &simple_post_core::domain::HttpMethod) -> String {
    m.as_str().to_string()
}
