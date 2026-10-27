//! `endpoint_*` 工具的输入形状。
//!
//! **转换规则住在 core**，不在这里：`simple_post_core::spec::input` 是那一处唯一的
//! 实现（`KeyValue` 的 `id` 缺失怎么办、`enabled` 缺省是什么、载荷长得像一份
//! Schema 怎么办），因为 `spec_upsert` 也要用同一套。规则写两遍就会漂移，而漂移
//! 的表现是「同一个字段在两条路上行为不同」——那是最难查的一类缺陷。
//!
//! 本文件留下的是**只属于 MCP 的那一部分**：给 AI 客户端看的 JSON Schema 字面量。

use serde_json::json;

// =========================================================================
// 从 core 转发过来的转换规则
// =========================================================================
//
// 一行键值对（`KeyValueInput` / `coerce_key_values`）不在这一组里：它们经
// `crate::mcp::key_value` 转发，那一处也服务 `api_call_endpoint` 的 `overrides`。

pub(super) use simple_post_core::spec::input::{
    build_auth, parse_body_mode, reject_schema_shaped_payload,
};
pub(super) use simple_post_core::spec::input::{AUTH_TYPE_VALUES, BODY_MODE_VALUES};

// =========================================================================
// 只属于 MCP 的那一部分：JSON Schema 字面量
// =========================================================================

/// OAuth2 载荷的 JSON Schema。两处用它（`endpoint_update` 的 `auth.oauth2` 与
/// `spec_upsert` 的 `endpoint.auth.oauth2`），故 AI 客户端在两条路上看到同一份形状。
///
/// `additionalProperties: false` 是承重的：`endpoint_update` 的 `auth` 上有它，
/// 而 agent 是这里最主要的外部写路径。漏登记一个字段不是「宽松一点」，而是那个字段
/// **根本传不进来**——一个仍然看着正确注册的工具却永远收不到配置。
///
/// 逐个属性的说明一律省掉，只留名字、类型与取值范围：OAuth2 的字段名是协议定的，
/// 说明文字对 agent 没有帮助，而每一个字节都要乘以会话数。
pub(in crate::mcp::project_ops) fn oauth2_config_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "OAuth2 settings. The token is fetched on demand and refreshed when it expires; authorization_code needs authCode pasted after the user authorises in a browser, and implicit has no token endpoint at all.",
        "properties": {
            "grantType": {
                "type": "string",
                "enum": ["authorization_code", "client_credentials", "password", "implicit"]
            },
            "tokenUrl": { "type": "string" },
            "clientId": { "type": "string" },
            "clientSecret": { "type": "string" },
            "clientAuth": {
                "type": "string",
                "enum": ["basic", "body"]
            },
            "scope": { "type": "string" },
            "authorizationUrl": { "type": "string" },
            "redirectUri": { "type": "string" },
            "codeVerifier": { "type": "string" },
            "authCode": { "type": "string" },
            "username": { "type": "string" },
            "password": { "type": "string" },
            "extraParams": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "key": { "type": "string" },
                        "value": { "type": "string" },
                        "enabled": { "type": "boolean" }
                    },
                    "required": ["key", "value"],
                    "additionalProperties": false
                }
            },
            "tokenLocation": { "type": "string", "enum": ["header", "query"] },
            "tokenParam": { "type": "string" }
        },
        "required": ["grantType"],
        "additionalProperties": false
    })
}
