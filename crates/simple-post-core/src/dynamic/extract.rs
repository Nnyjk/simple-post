//! 按 JSONPath 从响应体里抽一个值。

use serde_json::Value;

use crate::domain::DynamicErrorKind;

/// 按 JSONPath 从响应体里抽一个值。
///
/// 响应体必须能解析成 JSON（否则是 `not_json`）；路径没命中是 `no_match`；路径表达式本身
/// 非法是 `source_error`——那是定义坏了，不是数据的问题。
pub fn extract_value(body: &str, extract_path: &str) -> Result<Value, DynamicErrorKind> {
    let value: Value = serde_json::from_str(body).map_err(|_| DynamicErrorKind::NotJson)?;
    let path = if extract_path.trim().is_empty() {
        "$"
    } else {
        extract_path.trim()
    };
    let query =
        serde_json_path::JsonPath::parse(path).map_err(|_| DynamicErrorKind::SourceError)?;
    match query.query(&value).first() {
        Some(node) => Ok((*node).clone()),
        None => Err(DynamicErrorKind::NoMatch),
    }
}
