//! MCP 入参里的一行键值对。
//!
//! 域类型 [`KeyValue`](simple_post_core::domain::KeyValue) 的 `id` 与 `enabled` 没有
//! serde 默认值，于是最自然的写法 `{"key":"page","value":"1"}` 反序列化失败 —— 而
//! 这正是工具描述与 JSON Schema 文档化的形状。两处放宽的理由不同：
//!
//! - `id` 可选，缺失时另铸。纯簿记字段 —— UI 拿它在 React 里做行 key，请求组装不读。
//!   客户端真传了就保留，于是把上一行原样回传的编辑是就地更新，不会把它变成孤儿。
//! - `enabled` 可选，且**默认 `true`**，不是 `KeyValue` 自己的 `Default`（`false`）。
//!   存成禁用的请求头就是一个永远发不出去的请求头；agent 点名一个请求头就是要发它。
//!   显式 `false` 仍然照办，那对应「留着但先关掉」。
//!
//! `value` 用同样方式放宽，因为「故意发一个空请求头值」是真事，不是笔误。
//!
//! 这一份类型服务两处入参：`endpoint_create` / `endpoint_update` 的键值对数组，以及
//! `api_call_endpoint` / `api_resolve_endpoint` 的 `overrides`。两处放宽的规则相同，
//! 各写一份就会漂 —— 而漂掉的那一处表现为「工具描述说能传，调用却报字段缺失」。
use serde::Deserialize;
use serde_json::{json, Value};

use simple_post_core::domain::KeyValue;

/// agent 手写的一行 `KeyValue` 的线上形状。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyValueInput {
    #[serde(default)]
    pub(crate) id: Option<String>,
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) value: String,
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
    #[serde(default)]
    pub(crate) description: Option<String>,
}

impl KeyValueInput {
    pub(crate) fn into_key_value(self) -> KeyValue {
        let id = match self.id {
            Some(id) if !id.trim().is_empty() => id,
            _ => simple_post_core::domain::uid(),
        };
        KeyValue {
            id,
            key: self.key,
            value: self.value,
            enabled: self.enabled.unwrap_or(true),
            description: self.description,
        }
    }
}

/// 反序列化键值对数组，并补齐 `KeyValue` 要求、而调用方没有理由知道的那些簿记字段。
pub(crate) fn coerce_key_values(rows: Vec<KeyValueInput>) -> Vec<KeyValue> {
    rows.into_iter()
        .map(KeyValueInput::into_key_value)
        .collect()
}

/// 一行键值对的 JSON Schema。
///
/// `endpoint_create` / `endpoint_update` 与 `api_call_endpoint` 的 `overrides`
/// 都用 `#/$defs/keyValue` 引用它，故它住在共享的地方：两处各写一份，改了一处就有
/// 一处引着一个**没有定义**的 `$ref` —— 跟着 `$ref` 走的 agent 落到空处，只能自己猜
/// 这一行长什么样。
pub(crate) fn key_value_schema() -> Value {
    json!({
        "type": "object",
        "description": "One key/value row. `id` is optional bookkeeping (minted when omitted); `enabled` defaults to true.",
        "properties": {
            "key": { "type": "string", "description": "Header or parameter name." },
            "value": { "type": "string", "description": "Header or parameter value." },
            "enabled": { "type": "boolean", "description": "Send this row. Defaults to true; set false to keep it without sending." },
            "description": { "type": "string", "description": "Optional note about this row." },
            "id": { "type": "string", "description": "Stable row id, used to update a row in place. Omit when creating." }
        },
        "required": ["key", "value"],
        "additionalProperties": false
    })
}
