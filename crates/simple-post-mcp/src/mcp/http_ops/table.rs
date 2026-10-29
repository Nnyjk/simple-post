//! dispatch 表 —— 两个工具的登记项与 JSON Schema。
//!
//! 描述与 schema 是 `tools/list` 的全部内容，故两者都逐字写死在这里：
//! 描述只说「这个参数是什么」，规则与例子放 `simple-post://spec-authoring` 指南。

use serde_json::json;

use crate::mcp::ToolEntry;
use crate::tool_fn;

use super::call_endpoint::tool_api_call_endpoint;
use super::run_request::tool_api_run_request;

/// 两个工具的登记项。顺序即 `tools/list` 里的顺序。
pub(crate) fn table() -> Vec<ToolEntry> {
    vec![
        ToolEntry {
            name: "api_run_request",
            description:
                "Run an ad-hoc HTTP request, assembled by the same resolver the stored endpoints use: env variables, baseUrl, auth, params and body all follow the endpoint rules. A base URL is read from the (environment x module) cell, so env_id and module_id are needed.",
            schema: json!({
                "type": "object",
                "properties": {
                    "method": { "type": "string" },
                    "url": { "type": "string" },
                    "headers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key", "value"]
                        }
                    },
                    "params": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key", "value"]
                        }
                    },
                    "path_params": {
                        "type": "array",
                        "description": "Values for the path placeholders in `url` (e.g. the `id` of \"/api/users/{id}\"). They are substituted into the path itself, not appended to the query string.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["key", "value"]
                        }
                    },
                    "body": {
                        "type": "string",
                        "description": "The request payload to send, as text. For body_mode=json this is a JSON instance with values, not a JSON Schema."
                    },
                    "body_mode": {
                        "type": "string",
                        "enum": ["none", "json", "form", "raw", "multipart"],
                        "description": "How to frame the body. Defaults to raw when a body is given, none otherwise."
                    },
                    "auth": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["none", "bearer", "basic", "apikey", "oauth2"] },
                            "bearer": { "type": "string" },
                            "basic": {
                                "type": "object",
                                "properties": {
                                    "username": { "type": "string" },
                                    "password": { "type": "string" }
                                }
                            },
                            "apikey": {
                                "type": "object",
                                "properties": {
                                    "key": { "type": "string" },
                                    "value": { "type": "string" },
                                    "inLocation": { "type": "string", "enum": ["header", "query"] }
                                }
                            },
                            "oauth2": { "type": "object" }
                        },
                        "required": ["kind"]
                    },
                    "env_id": { "type": "string" },
                    "module_id": { "type": "string" },
                    "settings_override": {
                        "type": "object",
                        "description": "Outbound knobs for this call. `tls` takes snake_case keys: skip_verify, ca_certificate_path, client_certificate_path, client_key_path. Those four keys are one group: giving `tls` replaces the endpoint's own HTTPS settings as a whole, not field by field, so resend every key you want to keep."
                    },
                    "timeout_ms": { "type": "integer" },
                    "follow_redirects": { "type": "boolean" },
                    "max_response_size_kb": { "type": "integer" }
                },
                "required": ["method", "url"],
                "additionalProperties": true,
            }),
            func: tool_fn!(tool_api_run_request),
        },
        ToolEntry {
            name: "api_call_endpoint",
            description:
                "Resolve an endpoint by id (env vars, baseUrls, auth, params) and execute it. Returns the parsed response.",
            schema: json!({
                "type": "object",
                "properties": {
                    "endpoint_id": { "type": "string" },
                    // 覆盖值要写进 schema：路径参数缺值时发送那侧会拦下请求并说
                    // 「or pass overrides.pathParams when calling」，而调用方只能从
                    // schema 里知道这个入参的名字。schema 不写它，工具就在骗人。
                    "overrides": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string" },
                            "method": { "type": "string" },
                            "headers": {
                                "type": "array",
                                "items": { "$ref": "#/$defs/keyValue" }
                            },
                            "params": {
                                "type": "array",
                                "items": { "$ref": "#/$defs/keyValue" }
                            },
                            "pathParams": {
                                "type": "array",
                                "description": "Values for the `{placeholder}` segments in the url. They are \
substituted into the path itself, not appended to the query string. The key of each entry must match a \
placeholder name; a placeholder without an entry keeps its literal braces and blocks the send.",
                                "items": { "$ref": "#/$defs/keyValue" }
                            },
                            "moduleId": { "type": "string" }
                        },
                        "additionalProperties": true
                    },
                    "settings_override": {
                        "type": "object",
                        "description": "Outbound knobs for this call. `tls` takes snake_case keys: skip_verify, ca_certificate_path, client_certificate_path, client_key_path. Those four keys are one group: giving `tls` replaces the global default HTTPS settings as a whole, not field by field, so resend every key you want to keep."
                    }
                },
                "$defs": {
                    // `overrides` 的三个数组按 `#/$defs/keyValue` 引用条目形状。少了这个
                    // 定义，跟着 `$ref` 走的 agent 就落到一个空处，只能自己猜这一行长
                    // 什么样 —— 于是它多半猜成带 `id` 的域类型形状，而解析器要的是
                    // 「只要 key 与 value」。定义放在共享的地方（`mcp::key_value`），
                    // 与 `endpoint_create` / `endpoint_update` 用的是同一份。
                    "keyValue": crate::mcp::key_value::key_value_schema(),
                },
                "required": ["endpoint_id"],
                "additionalProperties": true,
            }),
            func: tool_fn!(tool_api_call_endpoint),
        },
    ]
}
