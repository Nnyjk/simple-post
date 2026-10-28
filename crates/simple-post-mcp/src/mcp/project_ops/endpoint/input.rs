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

// =========================================================================
// HTTPS 出站设置的 JSON Schema
// =========================================================================

/// HTTPS 出站设置的属性表。`endpoint_create` 的 `tls` 与 `endpoint_update` 的
/// `patch.tls` 描述的是同一份东西，故属性表只写一遍，两处引用它。
///
/// 键名是 camelCase（`skipVerify` / `caCertificatePath` / `clientCertificatePath` /
/// `clientKeyPath`），与 `simple_post_core::domain::TlsSettings` 自己的 serde 键名
/// 一致 —— 故这两个工具的 `tls` 载荷可以直接反序列化成那个类型，不需要转换。
///
/// 注意这与 `api_run_request` / `api_call_endpoint` 的 `settings_override.tls` 用的
/// **snake_case** 不同：那两个工具的 `settings_override` 里既有的键都是 snake_case，
/// 转换函数在 `mcp::http_ops::input::parse_tls_settings`。
fn tls_settings_properties() -> serde_json::Value {
    json!({
        "skipVerify": {
            "type": "boolean",
            "description": "Skip server certificate verification, hostname included."
        },
        "caCertificatePath": {
            "type": ["string", "null"],
            "description": "PEM file path of the trusted CA certificate(s)."
        },
        "clientCertificatePath": {
            "type": ["string", "null"],
            "description": "PEM file path of the client certificate (mTLS)."
        },
        "clientKeyPath": {
            "type": ["string", "null"],
            "description": "PEM file path of the client private key. Omit it to read the key from the client certificate file."
        }
    })
}

/// HTTPS 出站设置的 JSON Schema，`endpoint_create` 用它。
///
/// **这份设置整体取代全局默认**，不是逐字段覆盖：端点给了设置就用自己的那份
/// （含「什么都不改」的那份），没给才用全局默认。
///
/// **证书只存文件路径**，文件内容不进数据库；路径由**运行请求的那台机器**解析
/// ——桌面端是本机路径，`serve-web` / Docker 端是服务端容器里的路径。
pub(in crate::mcp::project_ops) fn tls_settings_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "This endpoint's own HTTPS outbound settings. They replace the global default settings as a whole, not field by field. Only file paths are stored, and the paths are resolved on the machine that runs the request.",
        "properties": tls_settings_properties(),
        "additionalProperties": false
    })
}

/// HTTPS 出站设置的 JSON Schema，`endpoint_update` 的 `patch.tls` 用它。
///
/// 与 [`tls_settings_schema`] 的差别只有一处：`null` 在这里是**清除端点自带的那份**，
/// 于是端点回到继承全局默认。
pub(in crate::mcp::project_ops) fn tls_settings_patch_schema() -> serde_json::Value {
    json!({
        "type": ["object", "null"],
        "description": "This endpoint's own HTTPS outbound settings. They replace the global default settings as a whole, not field by field. Only file paths are stored, and the paths are resolved on the machine that runs the request. null clears them, so the endpoint inherits the global default again.",
        "properties": tls_settings_properties(),
        "additionalProperties": false
    })
}
