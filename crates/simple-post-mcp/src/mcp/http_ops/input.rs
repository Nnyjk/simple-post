//! 工具入参 → 请求形状：键值对压平、设置覆盖、base URL 前置与 verb 还原。
//!
//! `flatten_string_map` 给 `response_parse` 也用，所以是 `pub(crate)`，并由门面
//! 重新导出在 `crate::mcp::http_ops::flatten_string_map`。

use serde_json::Value;

use super::RunRequestArgs;
use simple_post_core::domain::{Environment, HttpMethod};
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
                    out.push((pair[0].to_string(), pair[1].to_string()));
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

/// 从活的取值层读一个 base URL 前置上去。
///
/// `module_base_urls[def_id][module_id]` 是 URL 唯一住址（见
/// `variable_fill::resolve_base_url_with`），两个键都必须给，且**刻意没有兜底**：
/// 回退到 `env.base_urls` 那列死列、或「取第一个能用的键」，都等于把一个任意的
/// 服务怼到这次请求上。env、服务、模块、格子缺任意一个都表示「没有 base URL」，
/// 按调用方给的路径原样发出。
pub(super) fn apply_base_url(
    url: &str,
    env: Option<&Environment>,
    def_id: Option<&str>,
    module_id: Option<&str>,
) -> (String, String) {
    let (Some(env), Some(def_id), Some(module_id)) = (env, def_id, module_id) else {
        return (url.to_string(), String::new());
    };
    match env
        .module_base_urls
        .get(def_id)
        .and_then(|cells| cells.get(module_id))
    {
        Some(base) => {
            let combined = if url.starts_with('/') {
                format!("{}{}", base.trim_end_matches('/'), url)
            } else {
                format!("{}/{}", base.trim_end_matches('/'), url)
            };
            (combined, base.clone())
        }
        None => (url.to_string(), String::new()),
    }
}

/// `HttpMethod` 还原成线上字符串。
pub(super) fn format_method(m: &HttpMethod) -> String {
    m.as_str().to_string()
}
