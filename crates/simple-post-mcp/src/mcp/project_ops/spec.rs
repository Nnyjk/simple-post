//! `spec_upsert` —— 一次调用落一份完整的接口规范。
//!
//! ## 为什么要有这个工具
//!
//! 用 CRUD 粒度的工具建一份接口规范，agent 要自己编排七八次调用：建模块、建集合、
//! 建环境、写环境变量、写 base URL 那一格、建端点。而每一步的输出是下一步的输入，
//! 所以它们只能串行；每一步的参数拼错一个字母，得到的错误信息又不指向
//! 「你少写了环境」这件事。
//!
//! 这个工具把那份编排收在一处：调用方给一份声明式的规格，服务端在一个事务里把它
//! 变成库里那些行，缺什么建什么，已有的按名字复用。
//!
//! ## 编排住在 core，不在这里
//!
//! 落库的编排在 [`simple_post_core::spec::upsert`]，因为同一份规格的语义要在 REST
//! 路由、桌面 command 与这个工具三处一致。写在某一侧，另一侧就会静默少一半。
//! 本文件只做三件事：反序列化规格、过权限门、把结果与「还缺什么」组装成响应。

use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::error::AppError;
use simple_post_core::spec::upsert::EndpointSpec;

use super::endpoint::oauth2_config_schema;
use simple_post_core::spec::input::{AUTH_TYPE_VALUES, BODY_MODE_VALUES};

/// 请求体的 JSON Schema。
fn body_schema() -> Value {
    json!({
        "type": "object",
        "description": "The request body. `content` is the payload actually sent (a JSON *instance* when mode is \"json\"); `schema` is the documented parameter definition, never sent on the wire.",
        "properties": {
            "mode": { "type": "string", "enum": json!(BODY_MODE_VALUES) },
            "content": { "type": "string", "description": "The payload actually sent. For mode \"json\" it is a JSON instance with values, not a JSON Schema." },
            "schema": { "type": "string", "description": "JSON Schema describing the documented parameters. Never sent on the wire." }
        },
        "required": ["mode"],
        "additionalProperties": false
    })
}

/// 认证配置的 JSON Schema。
fn auth_schema() -> Value {
    json!({
        "type": "object",
        "description": "The auth scheme and its credentials. A scheme without its credential is rejected.",
        "properties": {
            "kind": { "type": "string", "enum": json!(AUTH_TYPE_VALUES) },
            "bearer": { "type": "string" },
            "basic": {
                "type": "object",
                "properties": {
                    "username": { "type": "string" },
                    "password": { "type": "string" }
                },
                "additionalProperties": false
            },
            "apikey": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "value": { "type": "string" },
                    "inLocation": { "type": "string", "enum": ["header", "query"] }
                },
                "additionalProperties": false
            },
            "oauth2": oauth2_config_schema()
        },
        "required": ["kind"],
        "additionalProperties": false
    })
}

/// 环境那一段的 JSON Schema。
///
/// 多词字段一律用 camelCase，与同一份 schema 里的 `endpoint` 段一致
/// （`pathParams` / `baseUrlOverride` / `preRequestScript`）。
///
/// `simple_post_core::spec::upsert::EnvironmentSpec` 同样收 snake_case 的别名，故服务端比
/// 这份 schema 宽：调用方写 `base_url` 一样收得到。那是宽容，不是两套契约 —— 规范拼写
/// 只有 camelCase 这一套，schema 写的与 core 的别名都收的正是它。
fn environment_schema() -> Value {
    json!({
        "type": "object",
        "description": "The environment this endpoint runs against.",
        "properties": {
            "name": { "type": "string", "description": "Environment name. Created when missing." },
            "id": { "type": "string", "description": "An existing environment id. Mutually exclusive with `name`." },
            "variables": {
                "type": "object",
                "description": "Variables to write. They are merged into the environment's variables; the ones the spec does not mention are kept.",
                "additionalProperties": { "type": "string" }
            },
            "disabledVariables": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Variable names to disable. A disabled variable is stored and shown but takes no part in substitution."
            },
            "baseUrl": {
                "type": "string",
                "description": "The backend api address of the (environment x module) cell. That cell has no other dimension to fall back on."
            },
            "baseUrlProxy": {
                "type": "string",
                "description": "The address the request actually goes to when it is reached through a proxy gateway. Omit it for no proxy."
            }
        },
        "additionalProperties": false
    })
}

/// 端点那一段的 JSON Schema。
fn endpoint_schema() -> Value {
    json!({
        "type": "object",
        "description": "The endpoint itself.",
        "properties": {
            "name": { "type": "string" },
            "method": {
                "type": "string",
                "description": "HTTP verb: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, or any RFC 9110 token."
            },
            "url": {
                "type": "string",
                "description": "Path only, e.g. \"/api/users/{id}\". The host comes from the environment's cell for this endpoint's module at send time."
            },
            "description": { "type": "string" },
            "docs": { "type": "string", "description": "Free-form documentation." },
            "notes": { "type": "string", "description": "Free-form notes." },
            "tags": { "type": "array", "items": { "type": "string" } },
            "params": {
                "type": "array",
                "description": "Query-string parameters.",
                "items": { "$ref": "#/$defs/keyValue" }
            },
            "path_params": {
                "type": "array",
                "description": "Values for the `{placeholder}` segments in the url. They are substituted into the path itself, not the query string; each key must match a placeholder name.",
                "items": { "$ref": "#/$defs/keyValue" }
            },
            "headers": {
                "type": "array",
                "description": "Request headers.",
                "items": { "$ref": "#/$defs/keyValue" }
            },
            "body": body_schema(),
            "auth": auth_schema(),
            "base_url_override": {
                "type": "string",
                "description": "Literal base URL for this endpoint, overriding the environment's cell for this endpoint's module."
            },
            "pre_request_script": {
                "type": "string",
                "description": "JavaScript run after variable resolution and before the request is sent. It may rewrite the request (pm.request)."
            },
            "test_script": {
                "type": "string",
                "description": "JavaScript run after the response arrives. It asserts (pm.test) and may write environment variables (pm.environment.set)."
            }
        },
        "required": ["name", "method", "url"],
        "additionalProperties": false
    })
}

/// `spec_upsert` 的 JSON Schema。
///
/// 参数的形状就是规格本身，故这份 schema 就是那份规格的说明书。**逐个属性的说明
/// 一律写得短**：详细的规则、例子与调用顺序在 `simple-post://spec-authoring` 那份
/// 按需读取的指南里，而 `tools/list` 的每一个字节都是每一次会话要付的钱。
///
/// 这份 schema 拆成几个小片段拼起来，不是一份 `json!` 字面量：单份字面量太深，
/// 超出 `json!` 宏的递归上限。
fn spec_schema() -> Value {
    json!({
        "type": "object",
        "description": "One interface spec, applied in a single call. Each part is \
    found by name and created when missing, so a second call reuses them all; only \
    if_exists=duplicate adds a second endpoint.",
        "properties": {
            "project_id": {
                "type": "string",
                "description": "The project the spec lands in. It must already exist."
            },
            "module": {
                "type": "string",
                "description": "Module name. Created when missing. Mutually exclusive with `module_id`."
            },
            "module_id": {
                "type": "string",
                "description": "An existing module id."
            },
            "collection_path": {
                "type": "string",
                "description": "Collection path relative to the module, e.g. \"订单/明细\". The collections on the way are created when missing. Mutually exclusive with `collection_id`."
            },
            "collection_id": {
                "type": "string",
                "description": "An existing collection id."
            },
            "environment": environment_schema(),
            "endpoint": endpoint_schema(),
            "if_exists": {
                "type": "string",
                "enum": ["reuse", "error", "duplicate", "update"],
                "description": "What to do when an endpoint with the same name, method and url already exists under that parent. \"reuse\" (default) returns it unchanged and writes nothing; \"error\" refuses; \"duplicate\" creates a second one; \"update\" overwrites the fields this spec names. It governs the endpoint only — the environment is always applied."            }        },
        "$defs": {
            "keyValue": crate::mcp::key_value::key_value_schema()
        },
        "required": ["project_id", "endpoint"],
        "additionalProperties": false,
    })
}

/// 登记这个工具。
pub(in crate::mcp::project_ops) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "spec_upsert",
        description: "Apply one interface spec in a single call: the module, the \
                       collections on its path, the environment and the endpoint. \
                       Each part is found by name and created when missing, so \
                       a second call reuses them all; only if_exists=duplicate adds a \
                       second endpoint. Use it instead of \
                       chaining the project/module/collection/env/baseUrl/endpoint tools, \
                       which costs one round trip per link in the chain.",
        schema: spec_schema(),
        func: tool_fn!(tool_spec_upsert),
    });
}

/// `spec_upsert` 的 handler。
pub(in crate::mcp::project_ops) async fn tool_spec_upsert(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let spec: EndpointSpec = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("spec_upsert 的规格反序列化失败：{e}")))?;

    // 权限门在写任何东西之前：规格指名了项目，而本调用会往那个项目里写。
    // 门没过就返回拒绝，库里一个字节都不动。
    crate::mcp::check_permission(&state.pool, spec.project_id.trim(), "spec_upsert").await?;

    // 落库与「还缺什么」由 `crate::spec::ops` 一处实现：REST 路由与桌面 command
    // 走的是同一段，故三个宿主回的是同一份形状，agent 与脚本不必各自再挖两级。
    let result = crate::spec::ops::upsert(&state.pool, &spec)?;

    Ok(CallToolResult::json(
        serde_json::to_value(&result)
            .map_err(|e| AppError::Validation(format!("spec_upsert 的结果序列化失败：{e}")))?,
    ))
}

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
