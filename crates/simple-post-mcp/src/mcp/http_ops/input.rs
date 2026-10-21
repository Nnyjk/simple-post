//! 工具入参 → 请求形状：键值对压平、鉴权与 body 模式解析、设置覆盖、verb 还原。
//!
//! `flatten_string_map` 给 `response_parse` 也用，所以是 `pub(crate)`，并由门面
//! 重新导出在 `crate::mcp::http_ops::flatten_string_map`。

use serde_json::Value;

use super::RunRequestArgs;
use simple_post_core::domain::{AuthConfig, BodyMode, KeyValue};
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
        other => Err(AppError::Validation(format!(
            "body_mode: unknown mode {other:?} (expected none|json|form|raw|multipart)"
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

/// `HttpMethod` 还原成线上字符串。
pub(super) fn format_method(m: &simple_post_core::domain::HttpMethod) -> String {
    m.as_str().to_string()
}
