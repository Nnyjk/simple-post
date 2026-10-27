//! MCP 入参里的一行键值对。
//!
//! **类型住在 core，不在这里**：`simple_post_core::spec::input::KeyValueInput` 是那一处
//! 唯一的实现（`id` 缺失时另铸、`enabled` 缺省为 `true`、`value` 可为空），因为
//! `endpoint_create` / `endpoint_update` 与 `spec_upsert` 必须对「一行键值对怎么读」
//! 有同一个答案。规则写两遍就会漂移，而漂移的表现是「同一个字段在两条路上行为不同」
//! —— 那是最难查的一类缺陷。
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
pub(crate) use simple_post_core::spec::input::{coerce_key_values, KeyValueInput};

use serde_json::{json, Value};

/// 一行键值对的 JSON Schema。
///
/// `endpoint_create` / `endpoint_update` / `spec_upsert` 与 `api_call_endpoint` 的
/// `overrides` 都用 `#/$defs/keyValue` 引用它，故它住在共享的地方：两处各写一份，
/// 改了一处就有一处引着一个**没有定义**的 `$ref` —— 跟着 `$ref` 走的 agent 落到空处，
/// 只能自己猜这一行长什么样。
///
/// 逐个属性的说明一律省掉，只留名字、类型与取值范围：这一行的形状是协议定的，说明
/// 文字对 agent 没有帮助，而 `tools/list` 的每一个字节都要乘以会话数。对象级的说明
/// 保留了两条承重的事实（`id` 可选、`enabled` 缺省为 `true`）。
pub(crate) fn key_value_schema() -> Value {
    json!({
        "type": "object",
        "description": "One key/value row. `id` is optional bookkeeping (minted when omitted); `enabled` defaults to true.",
        "properties": {
            "key": { "type": "string" },
            "value": { "type": "string" },
            "enabled": { "type": "boolean" },
            "description": { "type": "string" },
            "id": { "type": "string" }
        },
        "required": ["key", "value"],
        "additionalProperties": false
    })
}
