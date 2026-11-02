//! 8 个 `endpoint_*` 工具的 dispatch 登记与 JSON Schema。
//!
//! 登记表与 handler 分开放：handler 可以按读写职责各自搬走，而 `tools/list` 的顺序
//! 与 schema 字面量一行都不动。

use serde_json::json;

use super::input::{
    oauth2_config_schema, tls_settings_patch_schema, tls_settings_schema, AUTH_TYPE_VALUES,
    BODY_MODE_VALUES,
};
use super::read::{tool_endpoint_get, tool_endpoint_list, tool_endpoint_search};
use super::write::{
    tool_endpoint_create, tool_endpoint_delete, tool_endpoint_duplicate, tool_endpoint_move,
    tool_endpoint_update,
};
use crate::tool_fn;
pub(in crate::mcp::project_ops) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_list",
        description: "List endpoints in a project / module / collection. \
                       One of project_id, module_id or collection_id is \
                       required — an unscoped read spans every project and \
                       has no single project permission to check.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string", "description": "List every endpoint in this project." },
                "module_id": { "type": "string", "description": "List endpoints anywhere under this module." },
                "collection_id": { "type": "string", "description": "List this collection's direct endpoint children." }
            },
            "anyOf": [
                { "required": ["project_id"] },
                { "required": ["module_id"] },
                { "required": ["collection_id"] }
            ],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_get",
        description: "Fetch a single endpoint by id.",
        schema: json!({
            "type": "object",
            "properties": { "endpoint_id": { "type": "string" } },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_get),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_search",
        description: "Search endpoints by free-text query, method, or tags. \
                       One of project_id, module_id or collection_id is \
                       required — an unscoped search spans every project.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string", "description": "Search every endpoint in this project." },
                "module_id": { "type": "string", "description": "Search endpoints anywhere under this module." },
                "collection_id": { "type": "string", "description": "Search this collection's direct endpoint children." },
                "q": { "type": "string", "description": "Free text matched against endpoint name, url and description." },
                "method": { "type": "string", "description": "Keep only endpoints using this HTTP verb." },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Keep only endpoints carrying all of these tags." }
            },
            "anyOf": [
                { "required": ["project_id"] },
                { "required": ["module_id"] },
                { "required": ["collection_id"] }
            ],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_search),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_create",
        description: "Create an endpoint. Give it a parent: `collection_id` to put it \
                       inside an existing collection, `collection_path` (together with \
                       `module_id`) to create the collections on the way, or `module_id` \
                       to put it directly under a module. if_exists=reuse (the default) \
                       returns an existing endpoint with the same name, method and url \
                       unchanged. For a whole spec in one call, use spec_upsert.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": {
                    "type": "string",
                    "description": "Parent collection id. Required unless module_id or collection_path is given."
                },
                "module_id": {
                    "type": "string",
                    "description": "Parent module id. Required together with collection_path, since that path is relative to the module."
                },
                "collection_path": {
                    "type": "string",
                    "description": "Collection path relative to module_id, e.g. \"订单/明细\". The collections on the way are created when missing; existing ones with the same name are reused."
                },
                "if_exists": {
                    "type": "string",
                    "enum": ["reuse", "error", "duplicate"],
                    "description": "What to do when an endpoint with the same name, method and url already exists under that parent. \"reuse\" (default) returns it unchanged and writes nothing; \"error\" refuses; \"duplicate\" creates a second one."
                },                "name": { "type": "string" },
                "method": {
                    "type": "string",
                    "description": "HTTP verb: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, or any RFC 9110 token."
                },
                "url": {
                    "type": "string",
                    "description": "Path only, e.g. \"/api/users/{id}\". The host comes from the environment's cell for this endpoint's module at send time."
                },
                "headers": {
                    "type": "array",
                    "description": "Request headers.",
                    "items": { "$ref": "#/$defs/keyValue" }
                },
                "params": {
                    "type": "array",
                    "description": "Query-string parameters.",
                    "items": { "$ref": "#/$defs/keyValue" }
                },
                "path_params": {
                    "type": "array",
                    "description": "Values for the path placeholders in `url` (e.g. the `id` of \"/api/users/{id}\"). \
They are substituted into the path itself, not appended to the query string. The key of each entry must match a \
placeholder name in `url`; a placeholder without an entry keeps its literal braces and blocks the send.",
                    "items": { "$ref": "#/$defs/keyValue" }
                },
                "body_mode": {
                    "type": "string",
                    "enum": json!(BODY_MODE_VALUES),
                    "description": "How to interpret body_content when sending. \"none\" sends no \
body; \"raw\" sends the text verbatim; \"json\" sends a JSON instance; \"xml\" sends XML text; \
\"form\" sends x-www-form-urlencoded text; \"multipart\" sends a JSON array of parts, each \
{\"name\": string, \"value\": string, \"filename\"?: string, \"contentType\"?: string, \"base64\"?: \
string}, and it also replaces a Content-Type header you set; \"binary\" sends a JSON object \
{\"name\": string, \"size\": number, \"contentType\"?: string, \"base64\": string} as raw bytes."
                },
                "body_content": {
                    "type": "string",
                    "description": "The request payload actually sent on the wire, interpreted according to \
body_mode. This is the field for \"what to send when testing\"; the documented parameter definition \
goes in body_schema. For \"form\" it is a JSON array of rows [{\"key\", \"value\", \"enabled\"?, \
\"description\"?}]; for \"multipart\" it is a JSON array of parts; for \"binary\" it is a JSON \
object describing one file."
                },
                "body_schema": {
                    "type": "string",
                    "description": "JSON Schema describing the request body — the *documented* parameter \
definition of this endpoint. Never sent on the wire; it is what an OpenAPI export publishes. The value \
to send when testing goes in body_content, never here."
                },
                "auth_type": {
                    "type": "string",
                    "enum": json!(AUTH_TYPE_VALUES),
                    "description": "Auth scheme. \"none\" sends no credentials."
                },
                "auth_config": {
                    "type": "object",
                    "description": "Credentials for the chosen scheme: {\"bearer\":\"<token>\"}, \
{\"basic\":{\"username\":\"u\",\"password\":\"p\"}}, \
{\"apikey\":{\"key\":\"X-Api-Key\",\"value\":\"<key>\",\"inLocation\":\"header\"|\"query\"}}, or \
{\"oauth2\":{...}} (see auth.oauth2 in endpoint_update)."
                },
                "tags": { "type": "array", "items": { "type": "string" } },
                "base_url_override": {
                    "type": "string",
                    "description": "Literal base URL for this endpoint, overriding the environment's cell for the module this endpoint sits in."
                },
                "description": { "type": "string" },
                "docs": { "type": "string", "description": "Free-form documentation." },
                "notes": { "type": "string", "description": "Free-form notes." },
                "pre_request_script": {
                    "type": "string",
                    "description": "JavaScript run after variable resolution and before the request is sent. It may rewrite the request (pm.request)."
                },
                "test_script": {
                    "type": "string",
                    "description": "JavaScript run after the response arrives. It asserts (pm.test) and may write environment variables (pm.environment.set)."
                },
                "tls": tls_settings_schema()
            },
            "$defs": {
                "keyValue": crate::mcp::key_value::key_value_schema(),
            },
            "required": ["name", "method", "url"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_update",
        description: "Patch an endpoint's fields. Only the fields named in \
                       `patch` are changed; a malformed field is rejected \
                       with an error naming the field, and the whole patch \
                       is refused rather than partially applied.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "patch": {
                    "type": "object",
                    "description": "Fields to change. Omit a field to leave it as-is. `null` clears the field: `description`, `base_url_override`, `path_params`, `docs`, `notes`, `pre_request_script`, `test_script` and `tls` accept it. The other fields (`name`, `sort_order`, `method`, `url`, `params`, `headers`, `body`, `auth`, `tags`) reject null — send an empty value to clear them.",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": ["string", "null"] },
                        "sort_order": { "type": "integer" },
                        "method": {
                            "type": "string",
                            "description": "HTTP verb: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, or any RFC 9110 token."
                        },
                        "url": { "type": "string", "description": "Path only, e.g. \"/api/users/{id}\"." },
                        "params": {
                            "type": "array",
                            "description": "Replaces the whole query-parameter list.",
                            "items": { "$ref": "#/$defs/keyValue" }
                        },
                        "path_params": {
                            "type": "array",
                            "description": "Replaces the whole path-parameter list — the values for the `{placeholder}` \
segments in `url`. They are substituted into the path itself, not appended to the query string.",
                            "items": { "$ref": "#/$defs/keyValue" }
                        },
                        "headers": {
                            "type": "array",
                            "description": "Replaces the whole header list.",
                            "items": { "$ref": "#/$defs/keyValue" }
                        },
                        "body": {
                            "type": "object",
                            "description": "Replaces the whole body. `content` is the payload actually sent (a JSON *instance* when mode is \"json\"); `schema` is the JSON Schema describing the documented parameters, never sent on the wire.",
                            "properties": {
                                "mode": { "type": "string", "enum": json!(BODY_MODE_VALUES) },
                                "content": {
                                    "type": "string",
                                    "description": "The request payload actually sent. For mode \"json\" this is a JSON instance with values, not a JSON Schema — the definition goes in `schema`. For mode \"form\" it is a JSON array of rows [{\"key\", \"value\", \"enabled\"?, \"description\"?}]; for mode \"multipart\" it is a JSON array of parts; for mode \"binary\" it is a JSON object describing one file."
                                },
                                "schema": {
                                    "type": ["string", "null"],
                                    "description": "JSON Schema describing the request body. Never sent on the wire. null clears it."
                                }
                            },
                            "required": ["mode"],
                            "additionalProperties": false
                        },
                        "auth": {
                            "type": "object",
                            "description": "Replaces the whole auth config. A scheme without its credential is rejected.",
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
                        },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "base_url_override": { "type": ["string", "null"] },
                        "docs": { "type": ["string", "null"] },
                        "notes": { "type": ["string", "null"] },
                        "pre_request_script": {
                            "type": ["string", "null"],
                            "description": "JavaScript run after variable resolution and before the request is sent. It may rewrite the request (pm.request). null clears it."
                        },
                        "test_script": {
                            "type": ["string", "null"],
                            "description": "JavaScript run after the response arrives. It asserts (pm.test) and may write environment variables (pm.environment.set). null clears it."
                        },
                        "tls": tls_settings_patch_schema()
                    },
                    "additionalProperties": false
                }
            },
            "$defs": {
                "keyValue": crate::mcp::key_value::key_value_schema(),
            },
            "required": ["endpoint_id", "patch"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_delete",
        description: "Delete an endpoint by id.",
        schema: json!({
            "type": "object",
            "properties": { "endpoint_id": { "type": "string" } },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_duplicate",
        description: "Duplicate an endpoint (optionally with a new name).",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "name": { "type": "string" }
            },
            "required": ["endpoint_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_duplicate),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_move",
        description: "Move an endpoint to a new parent / position. The new parent \
                       may be a collection or a module id.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "new_collection_id": {
                    "type": "string",
                    "description": "New parent id — a collection, or a module to place the endpoint directly under one."
                },
                "new_sort_order": { "type": "integer" }
            },
            "required": ["endpoint_id", "new_collection_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_move),
    });
}
