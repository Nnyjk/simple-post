//! 值的序列化：`value_mode` 的形状。

use serde_json::Value;

use crate::domain::ValueMode;

/// 把抽到的值变成替换用的文本。
///
/// - `json`：始终是 JSON 序列化，故标量带引号（`"A001"`）。
/// - `auto` 与 `text`：对象与数组给裸 JSON 片段，标量给纯文本。两者输出**完全一致**——
///   `text` 是用户的声明（值会被放进请求体的字符串位置，转义由用户承担），不是另一种形态。
///
/// 替换环节不做任何转义。
pub fn serialize_value(value: &Value, mode: ValueMode) -> String {
    match mode {
        ValueMode::Json => serde_json::to_string(value).unwrap_or_default(),
        ValueMode::Auto | ValueMode::Text => match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            other => serde_json::to_string(other).unwrap_or_default(),
        },
    }
}

/// 覆盖值是用户手打的文本：能解析成 JSON 就用它，解析不了就当成一个字符串。
///
/// 这样用户写 `A001` 得到 `A001`，写 `[{"code":"A001"}]` 得到数组。
pub fn parse_raw(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}
