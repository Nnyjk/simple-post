//! MCP resources —— 给 AI 客户端读的静态说明。
//!
//! 工具描述是**逐个工具**的契约；这份 resource 是**跨工具**的那份：把「创建一个
//! 接口规范」要按什么顺序做、哪些参数能省掉一轮、响应里那几个字段是什么意思，
//! 一次说清。
//!
//! 为什么放在 resource 而不是工具描述里：这段内容对每个工具都只说得上一点点，
//! 塞进 52 条工具描述里就是 52 份重复，而 tools/list 的体积是要为每一次会话付钱的。
//! resource 是**按需读**的 —— 客户端在动手之前拉一次，agent 就拿到了完整的上下文。

use serde_json::{json, Value};

/// 这份 resource 的 uri。
pub const SPEC_AUTHORING_URI: &str = "simple-post://spec-authoring";

/// `resources/list` 的返回。
pub fn list() -> Value {
    json!({
        "resources": [
            {
                "uri": SPEC_AUTHORING_URI,
                "name": "Interface spec authoring guide",
                "description": "How to build one interface spec with as few calls as possible: \
    the one-call spec_upsert shape, the manual call order, the full endpoint_create \
    argument shape, and what the extra response fields mean.",
                "mimeType": "application/json"
            }
        ]
    })
}

/// `resources/read` 的返回。uri 不认识时返回 `None`，由调用方报「没有这个资源」。
pub fn read(uri: &str) -> Option<Value> {
    if uri != SPEC_AUTHORING_URI {
        return None;
    }
    let text =
        serde_json::to_string_pretty(&guide()).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    Some(json!({
        "contents": [
            {
                "uri": SPEC_AUTHORING_URI,
                "mimeType": "application/json",
                "text": text
            }
        ]
    }))
}

/// 那份指南本身。
///
/// 里面写的每一条都必须是当前实现的事实 —— 一份过期的指南比没有指南更糟，
/// 它会让 agent 按不存在的参数去调，然后花好几轮去发现。
fn guide() -> Value {
    json!({
        "purpose": "Build one interface spec with as few tool calls as possible.",

        "fastPath": {
            "tool": "spec_upsert",
            "note": "One call lands the whole spec: the module, the collections on the path, the environment, the service and the endpoint, in one transaction. Each part is found by name and created when missing, so applying the same spec twice changes nothing.",
            "arguments": {
                "project_id": "<项目 id；必须已经存在>",
                "module": "<模块名，不存在就建；用 module_id 指认已有模块>",
                "collection_path": "<集合路径，如 订单/明细；不写就挂在模块下>",
                "environment": {
                    "name": "<环境名，不存在就建；用 id 指认已有环境>",
                    "variables": { "<变量名>": "<值>" },
                    "base_url": "<这个环境 × 服务 × 模块 那一格的 URL>",
                    "activate": true
                },
                "service": {
                    "name": "<服务名，不存在就建；用 id 指认已有服务>",
                    "default": true
                },
                "endpoint": {
                    "name": "<接口名>",
                    "method": "GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS",
                    "url": "<路径，如 /api/orders；host 在发送时由环境的服务 URL 决定>",
                    "params": [{ "key": "<参数名>", "value": "<值>", "enabled": true }],
                    "path_params": [{ "key": "<路径占位符名，如 id>", "value": "<值>", "enabled": true }],
                    "headers": [{ "key": "<头名>", "value": "<值>", "enabled": true }],
                    "body": {
                        "mode": "none | json | form | multipart | raw",
                        "content": "<这一次发出去的载荷；json 模式下是一份 JSON 实例>",
                        "schema": "<这个接口接受哪些参数的文档定义（JSON Schema）；永远不上线>"
                    },
                    "auth": { "kind": "none | bearer | basic | apikey | oauth2", "bearer": "<token>" },
                    "tags": ["<标签>"],
                    "pre_request_script": "<发请求之前跑的 JavaScript>",
                    "test_script": "<响应到达之后跑的 JavaScript>"
                },
                "if_exists": "reuse | error | duplicate | update"
            },
            "ifExists": {
                "reuse": "The default. An endpoint with the same name, method and url already under that parent is returned unchanged; nothing is written.",
                "error": "Refuses when such an endpoint already exists.",
                "duplicate": "Builds a second endpoint with the same identity.",
                "update": "Overwrites the fields this spec names and leaves the others alone. The name, the verb and the path are the endpoint's identity, so they never change here.",
                "scope": "if_exists governs the endpoint only. The environment and the service sections are always applied."
            }
        },

        "calls": [
            {
                "step": 1,
                "tool": "project_create",
                "arguments": { "name": "<项目名>", "if_exists": "reuse" },
                "note": "if_exists=reuse returns the existing project with that name instead of creating a second one."
            },
            {
                "step": 2,
                "tool": "module_create",
                "arguments": { "project_id": "<项目 id>", "name": "<模块名>", "if_exists": "reuse" },
                "note": "Modules can only sit at the top level of a project, so a module is addressed by project plus name."
            },
            {
                "step": 3,
                "tool": "base_url_def_create",
                "arguments": { "project_id": "<项目 id>", "name": "<服务名>", "if_exists": "reuse" },
                "note": "A service belongs to the project, not to a module. if_exists=reuse returns the existing service with that name instead of creating a second one. If the project has no default service (★), call base_url_def_set_default once."
            },
            {
                "step": 4,
                "tool": "env_create",
                "arguments": { "project_id": "<项目 id>", "name": "<环境名>", "if_exists": "reuse" },
                "note": "A new environment is never the active one. Call env_set_active when it should be."
            },
            {
                "step": 5,
                "tool": "env_update",
                "arguments": { "env_id": "<环境 id>", "patch": { "variables": { "<变量名>": "<值>" } } },
                "note": "patch.variables replaces the whole variable map — send every variable the environment should end up with, in one call. env_set_var changes a single one."
            },
            {
                "step": 6,
                "tool": "env_set_baseurl",
                "arguments": { "env_id": "<环境 id>", "definition_id": "<服务 id>", "module_id": "<模块 id>", "url": "<base url>" },
                "note": "The URL lives in the (environment x service x module) cell. That cell never inherits, so a cell that is not written means the request goes out without a base URL."
            },
            {
                "step": 7,
                "tool": "endpoint_create",
                "arguments": {
                    "module_id": "<模块 id>",
                    "collection_path": "<集合路径，如 订单/明细>",
                    "name": "<接口名>",
                    "method": "GET",
                    "url": "<路径，如 /api/orders>"
                },
                "note": "One call creates the endpoint and every collection on its path. The full argument shape is in endpointTemplate."
            }
        ],

        "callNote": "The sequence above is the manual path, for when each step has to be its own call. spec_upsert does all of it in one call — use it unless a step has to be inspected or retried on its own.",

        "endpointTemplate": {
            "module_id": "<模块 id>",
            "collection_path": "<集合路径，如 订单/明细；段之间一个斜杠>",
            "name": "<接口名>",
            "method": "GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS",
            "url": "<路径，如 /api/orders；host 在发送时由环境的服务 URL 决定>",
            "params": [{ "key": "<参数名>", "value": "<值>", "enabled": true }],
            "headers": [{ "key": "<头名>", "value": "<值>", "enabled": true }],
            "body_mode": "none | json | form | multipart | raw",
            "body_content": "<这一次发出去的载荷；json 模式下是一份 JSON 实例>",
            "body_schema": "<这个接口接受哪些参数的文档定义（JSON Schema）；永远不上线>",
            "auth_type": "none | bearer | basic | apikey | oauth2",
            "auth_config": { "bearer": "<token>" },
            "environment_id": "<环境 id>",
            "base_url_definition_id": "<服务 id>",
            "tags": ["<标签>"],
            "pre_request_script": "<发请求之前跑的 JavaScript>",
            "test_script": "<响应到达之后跑的 JavaScript>",
            "if_exists": "reuse"
        },

        "responseFields": {
            "created": "true = this call created the endpoint; false = an endpoint with the same name, method and url already existed and was returned unchanged.",
            "created_collections": "One entry per level of collection_path, each with its own id and a created flag. A level that already existed has created=false.",
            "missing": "What is still missing before the request can go out: a base URL cell that has no value, or a {{variable}} the environment does not define. An empty list means the endpoint is ready to send.",
            "spec_upsert": "spec_upsert answers with the same three ideas: `module` and `collections` carry a `created` flag each, `environment` and `service` carry their ids, and `endpoint` carries the endpoint itself plus its `created` and `updated` flags and a `missing` list."
        },

        "rules": [
            "A base URL lives in the (environment x service x module) cell. That cell never inherits from the project or from a sibling module.",
            "body_content is the payload that goes on the wire; body_schema is the documented parameter definition. They are different things — a JSON Schema written into body_content is rejected, and the error names body_schema.",
            "body_mode says how body_content is framed: \"none\" sends no body, \"raw\" sends the text verbatim, \"json\" sends a JSON instance, \"form\" sends urlencoded text, and \"multipart\" sends a JSON array of parts — each {\"name\": string, \"value\": string, \"filename\"?: string, \"contentType\"?: string}, where contentType defaults to application/octet-stream for a part that has a filename.",
            "A {{var}} in the url, params, headers, body or auth is replaced from the environment's variables. A name that is not there is listed in missing.",
            "Path parameters are separate from query parameters: `params` goes into the query string, `path_params` fills the `{placeholder}` segments in the path itself, and the key of each path parameter must match a placeholder name.",
            "Dynamic variables are global and their values are resolved before substitution, so a name that is a dynamic variable is not reported in missing.",
            "An endpoint without a collection is fine: give module_id only and it sits directly under the module.",
            "endpoint_update changes only the fields its patch names. Use it to change an existing endpoint; endpoint_create with if_exists=reuse never writes anything.",
            "spec_upsert merges the variables it names into the environment and leaves the other variables alone. It never creates a project and never changes a project's permission tier.",
            "An auth scheme without its credential is rejected: kind=bearer needs a bearer token, kind=basic needs a username, kind=apikey needs a key name.",
            "The OAuth2 config accepts grantType (authorization_code | client_credentials | password | implicit), tokenUrl, clientId, clientSecret, clientAuth (basic | body), scope, authorizationUrl, redirectUri, codeVerifier, authCode, username, password, extraParams, tokenLocation (header | query) and tokenParam. The token is fetched on demand and refreshed when it expires; authorization_code needs authCode pasted after the user authorises in a browser, and implicit has no token endpoint at all."
        ]
    })
}
