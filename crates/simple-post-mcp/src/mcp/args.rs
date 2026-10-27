//! 参数辅助函数 —— 每个工具实现都会用。

use serde_json::Value;

/// 取必填字符串字段。缺失或类型不对 → error。
pub fn arg_str<'a>(
    args: &'a Value,
    key: &str,
) -> Result<&'a str, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-string '{key}'"))
    })
}

/// 取可选字符串字段。缺失或 null 都返回 `None`。
pub fn arg_str_opt(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(String::from)
}

/// 取必填 u64 字段。
pub fn arg_u64(args: &Value, key: &str) -> Result<u64, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-u64 '{key}'"))
    })
}

/// 取可选 u64 字段。
pub fn arg_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

/// 取必填布尔字段。
pub fn arg_bool(args: &Value, key: &str) -> Result<bool, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_bool).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-bool '{key}'"))
    })
}

/// 取可选布尔字段，缺失时为 `false`。
pub fn arg_bool_opt(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}
