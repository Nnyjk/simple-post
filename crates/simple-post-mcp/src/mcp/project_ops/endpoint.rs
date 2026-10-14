//! `endpoint_*` MCP 工具 —— 端点 CRUD、搜索、复制、移动，共 8 个。
//!
//! 只服务于这些工具的私有辅助函数也放在这里：`parse_body_mode` / `build_auth` 负责
//! `endpoint_create` 的输入解析；过门时用 `simple_post_core::db::nodes::root_id_of`
//! 解析归属项目 —— 一次索引读。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, arg_str_opt, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{
    EndpointDetail, EndpointDetailPatch, EndpointResource, RequestBody, ResourceNode,
    ResourceNodeType,
};
use simple_post_core::error::AppError;

/// node 与其请求配置合成一个对象返回。
///
/// AI 客户端问「把这个端点给我」要的是 method 与 url，而它们现在在
/// `endpoint_details` 里；只回 node 只会得到一个名字和一个类型。
fn endpoint_resource(
    pool: &simple_post_core::db::DbPool,
    node_id: &str,
) -> Result<EndpointResource, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        let node = simple_post_core::db::nodes::get(c, node_id)?;
        let detail = simple_post_core::db::endpoint_details::get(c, node_id)?;
        Ok(EndpointResource { node, detail })
    })
}

/// 给端点 node 列表配上各自的配置，保持调用方给定的 node 顺序。
fn endpoint_resources(
    pool: &simple_post_core::db::DbPool,
    nodes: Vec<ResourceNode>,
) -> Result<Vec<EndpointResource>, AppError> {
    let ids = nodes.iter().map(|n| n.id.clone()).collect::<Vec<String>>();
    let details = simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::endpoint_details::list_by_node_ids(c, &ids)
    })?;
    let by_id = details
        .into_iter()
        .map(|d| (d.node_id.clone(), d))
        .collect::<std::collections::HashMap<_, _>>();
    Ok(nodes
        .into_iter()
        .filter_map(|node| {
            by_id.get(&node.id).map(|detail| EndpointResource {
                node,
                detail: detail.clone(),
            })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointCreateArgs {
    /// 父集合。端点现在可以直接挂在模块下，所以它与 `module_id` 二选一即可。
    #[serde(default)]
    pub collection_id: Option<String>,
    /// 父模块，给不需要自己集合的端点用。
    #[serde(default)]
    pub module_id: Option<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub params: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub body_mode: Option<String>,
    #[serde(default)]
    pub body_content: Option<String>,
    /// 可选的 body schema 提示，原样存储，不参与发包。
    #[serde(default)]
    pub body_schema: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// 要绑定的环境。建时就收下，新建的端点直接指向正确的环境，省掉第二次调用。
    #[serde(default)]
    pub environment_id: Option<String>,
    /// 该端点据以解析的服务（`base_url_definitions` 的一行）。`None` = 沿用项目 ★。
    #[serde(default)]
    pub base_url_definition_id: Option<String>,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// 生成唯一的端点 id。
///
/// 绝不能用 `ep_` + `now_ms()`：那是毫秒时钟，同一毫秒内两次 `endpoint_create` 会
/// 得到同一个主键，第二次直接撞 `nodes.id` 约束。AI 客户端成批循环创建时必然踩到。
///
/// `domain::uid()` 是本 crate 的 id 唯一来源：纳秒 + 进程内计数器，进程内不撞、重启
/// 也不重复前缀。保留可读的 `ep_` 前缀，与 `col_` / `mod_` / `proj_` 一致。
fn mint_endpoint_id() -> String {
    let uid = simple_post_core::domain::uid();
    // `uid()` 给出 `sp_<hex>`；只把通用前缀换成端点的，别在这里重新实现时钟 ——
    // crate 里应当只有一处 id 来源。
    uid.replacen("sp_", "ep_", 1)
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointMoveArgs {
    pub endpoint_id: String,
    pub new_collection_id: String,
    #[serde(default)]
    pub new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointSearchArgs {
    #[serde(default)]
    pub project_id: Option<String>,
    /// 收窄到模块的整棵子树。由过门与结果收窄消费，不进 SQL —— `nodes::search` 是
    /// 全表匹配 name / url / description，子树过滤只能在它返回之后做。
    #[serde(default)]
    pub module_id: Option<String>,
    /// 收窄到某个集合的直接子节点。
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointGetArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDeleteArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDuplicateArgs {
    pub endpoint_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
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
                       required — an unscoped search spans every project and \
                       has no single project permission to check.",
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
        description: "Create a new endpoint inside a collection, or directly \
                       under a module (pass module_id when no collection is \
                       wanted).",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": {
                    "type": "string",
                    "description": "Parent collection id. Required unless module_id is given."
                },
                "module_id": {
                    "type": "string",
                    "description": "Parent module id, to create the endpoint directly under a module."
                },
                "name": { "type": "string" },
                "method": {
                    "type": "string",
                    "description": "HTTP verb. Standard verbs: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS. Any RFC 9110 token (e.g. PROPFIND) is also accepted."
                },
                "url": {
                    "type": "string",
                    "description": "Path only, e.g. \"/api/users/{id}\". The host is resolved at send time from the active environment's service URLs."
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
                "body_mode": {
                    "type": "string",
                    "enum": json!(BODY_MODE_VALUES),
                    "description": "How to interpret body_content when sending. \"none\" sends no body."
                },
                "body_content": {
                    "type": "string",
                    "description": "Raw request body, interpreted according to body_mode (JSON text when mode is \"json\")."
                },
                "body_schema": {
                    "type": "string",
                    "description": "Optional schema hint stored alongside the body; not sent on the wire."
                },
                "auth_type": {
                    "type": "string",
                    "enum": json!(AUTH_TYPE_VALUES),
                    "description": "Auth scheme. \"none\" sends no credentials."
                },
                "auth_config": {
                    "type": "object",
                    "description": "Scheme-specific credentials: {\"bearer\":\"<token>\"} for bearer, {\"basic\":{\"username\":\"u\",\"password\":\"p\"}} for basic, {\"apikey\":{\"key\":\"<header name>\",\"value\":\"<key>\",\"inLocation\":\"header\"|\"query\"}} for apikey."
                },
                "tags": { "type": "array", "items": { "type": "string" } },
                "environment_id": {
                    "type": "string",
                    "description": "Environment to bind. Omit for none."
                },
                "base_url_definition_id": {
                    "type": "string",
                    "description": "Service this endpoint resolves against (a base_url_definitions id). Omit to inherit the project's default service."
                },
                "base_url_override": {
                    "type": "string",
                    "description": "Literal base URL for this endpoint, overriding the environment's value for the chosen service."
                },
                "description": { "type": "string" },
                "docs": { "type": "string", "description": "Free-form documentation." },
                "notes": { "type": "string", "description": "Free-form notes." }
            },
            "$defs": {
                "keyValue": {
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
                }
            },
            "required": ["name", "method", "url"],
            "additionalProperties": true,
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
                    "description": "Fields to change. Omit a field to leave it as-is; send null to clear it.",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": ["string", "null"] },
                        "sort_order": { "type": "integer" },
                        "method": {
                            "type": "string",
                            "description": "HTTP verb. Standard verbs: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS. Any RFC 9110 token is also accepted."
                        },
                        "url": { "type": "string", "description": "Path only, e.g. \"/api/users/{id}\"." },
                        "params": {
                            "type": "array",
                            "description": "Replaces the whole query-parameter list.",
                            "items": { "$ref": "#/$defs/keyValue" }
                        },
                        "headers": {
                            "type": "array",
                            "description": "Replaces the whole header list.",
                            "items": { "$ref": "#/$defs/keyValue" }
                        },
                        "body": {
                            "type": "object",
                            "description": "Replaces the whole body.",
                            "properties": {
                                "mode": { "type": "string", "enum": json!(BODY_MODE_VALUES) },
                                "content": { "type": "string" },
                                "schema": { "type": ["string", "null"] }
                            },
                            "required": ["mode"],
                            "additionalProperties": false
                        },
                        "auth": {
                            "type": "object",
                            "description": "Replaces the whole auth config.",
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
                                }
                            },
                            "required": ["kind"],
                            "additionalProperties": false
                        },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "environment_id": { "type": ["string", "null"] },
                        "base_url_definition_id": { "type": ["string", "null"] },
                        "base_url_override": { "type": ["string", "null"] },
                        "docs": { "type": ["string", "null"] },
                        "notes": { "type": ["string", "null"] }
                    },
                    "additionalProperties": false
                }
            },
            "$defs": {
                "keyValue": {
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
                }
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

pub(super) async fn tool_endpoint_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str_opt(&args, "collection_id");
    let project_id = arg_str_opt(&args, "project_id");
    let module_id = arg_str_opt(&args, "module_id");

    // 按「调用方点名的东西真正归谁」过门。`collection_id` / `module_id` 指的是
    // *node*，所以 `nodes::root_id_of` 一次索引读就能拿到它的项目；裸 `project_id`
    // 本身就是答案。
    //
    // 写死字面量 "global" 不是项目 id（`agent_permissions.project_id` 是指向
    // `projects(id)` 的外键），查不到行就退回 `Permission::default()`。更糟的是按
    // 集合或模块收窄时根本没有归属项目可查，真实拥有者的权限一次也没被读过 —— 只读
    // 项目照样能被这种收窄调用读走。解析出真实归属才补上这一层。
    //
    // 一个范围都不给时，这次读确实横跨所有项目，没有单一项目可依。那就明确拒绝，
    // 而不是编一个 id：跨项目的读，按项目那扇门表达不了；拿一个项目的授权去返回
    // 另一个项目的数据，比说不行更糟。
    let owner_project = match collection_id.as_deref().or(module_id.as_deref()) {
        Some(node_id) => Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::root_id_of(c, node_id)
        })?),
        None => project_id.clone(),
    };
    let Some(owner_project) = owner_project else {
        return Err(AppError::Validation(
            "endpoint_list needs a scope: pass project_id, module_id or \
             collection_id. An unscoped read spans every project, and the \
             per-project permission gate has no single project to check."
                .into(),
        ));
    };
    crate::mcp::check_permission(&state.pool, &owner_project, "endpoint_list").await?;

    // `module_id` 收窄到模块整棵子树：节点的父链现在是同构的 —— 除模块外，每个节点
    // 都有节点父级。
    let scope = collection_id
        .as_deref()
        .map(|c| ("collection", c.to_string()))
        .or_else(|| module_id.clone().map(|m| ("module", m.to_string())))
        .or_else(|| project_id.clone().map(|p| ("project", p)));

    let rows = match scope {
        Some(("collection", cid)) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_children(c, &cid, Some(ResourceNodeType::Endpoint))
        })?,
        Some(("module", mid)) => {
            let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&mid))
            })?;
            let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
            let nodes = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::list_by_root_and_type(
                    c,
                    &owner_project,
                    ResourceNodeType::Endpoint,
                )
            })?;
            nodes
                .into_iter()
                .filter(|n| in_subtree.contains(&n.id))
                .collect()
        }
        Some((_, pid)) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(c, &pid, ResourceNodeType::Endpoint)
        })?,
        // 走不到：`scope` 为 `None` 只在什么都没点名时发生，而那种情况已被上面
        // 「没有归属可过门」的检查拒掉了。
        None => Vec::new(),
    };

    let endpoints: Vec<ResourceNode> = rows
        .into_iter()
        .filter(|n| n.kind == ResourceNodeType::Endpoint)
        .collect();
    Ok(CallToolResult::json(endpoint_resources(
        &state.pool,
        endpoints,
    )?))
}

pub(super) async fn tool_endpoint_get(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointGetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_get args: {e}")))?;
    let ep = endpoint_resource(&state.pool, &a.endpoint_id)?;
    crate::mcp::check_permission(&state.pool, &ep.node.root_id, "endpoint_get").await?;
    Ok(CallToolResult::json(ep))
}

pub(super) async fn tool_endpoint_search(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointSearchArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_search args: {e}")))?;

    // 归属解析与 `endpoint_list` 完全一致。字面量 "global" 不是项目 id（那一列是指向
    // `projects(id)` 的外键），查不到行就静默退回默认值；按集合或模块收窄时更糟，
    // 真实拥有者的权限根本没被查过。
    //
    // 一个范围都不给就横跨项目，没有单一归属可授权，同样明确拒绝而不是编 id。
    // 另外注意这里传的是**本工具**的名字：传别的名字会让针对搜索的权限改动被拿去
    // 比另一行。
    let owner_project = match a.collection_id.as_deref().or(a.module_id.as_deref()) {
        Some(node_id) => Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::root_id_of(c, node_id)
        })?),
        None => a.project_id.clone(),
    };
    let Some(owner_project) = owner_project else {
        return Err(AppError::Validation(
            "endpoint_search needs a scope: pass project_id, module_id or \
             collection_id. An unscoped search spans every project, and the \
             per-project permission gate has no single project to check."
                .into(),
        ));
    };
    crate::mcp::check_permission(&state.pool, &owner_project, "endpoint_search").await?;

    // `method` 与 `tags` 住在 `endpoint_details`，所以自由文本匹配与这两个过滤都放在
    // 收窄到端点 node 之后 —— SQL 层只匹配 name / url / description。
    let mut nodes = match a.q.as_deref().filter(|q| !q.is_empty()) {
        Some(q) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::search(c, q)
        })?,
        None => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(
                c,
                &owner_project,
                ResourceNodeType::Endpoint,
            )
        })?,
    };
    // `module_id` / `collection_id` 范围必须也收窄自由文本那一支，不只是无过滤那一支
    // —— 否则门查的是模块所属项目，返回的结果却横跨所有项目，正是这道门要挡的
    // 跨项目渗漏。
    if a.q.as_deref().filter(|q| !q.is_empty()).is_some() {
        if let Some(scope_id) = a.collection_id.as_deref().or(a.module_id.as_deref()) {
            let scope_id = scope_id.to_string();
            let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&scope_id))
            })?;
            let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
            nodes.retain(|n| in_subtree.contains(&n.id));
        } else {
            nodes.retain(|n| n.root_id == owner_project);
        }
    }
    nodes.retain(|n| n.kind == ResourceNodeType::Endpoint);
    let mut out = endpoint_resources(&state.pool, nodes)?;

    if let Some(m) = a.method.as_deref() {
        let want = simple_post_core::domain::parse_http_method(m)?;
        out.retain(|r| r.detail.method == want);
    }
    if let Some(tags) = a.tags.as_deref() {
        out.retain(|r| tags.iter().all(|t| r.detail.tags.contains(t)));
    }
    Ok(CallToolResult::json(out))
}

pub(super) async fn tool_endpoint_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }
    // 先验 HTTP 动词再碰权限表：输入级错误很便宜，不该被误报成权限拒绝。
    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    // 端点的父级是集合，或者调用方没建文件夹时直接是模块。`collection_id` 与
    // `module_id` 是同一关系的两个名字（`collection_create` 用的也是同一形状），
    // 取客户端给的那个，不成对的组合交给约束矩阵去拒。
    let parent_id = a
        .collection_id
        .clone()
        .or_else(|| a.module_id.clone())
        .ok_or_else(|| {
            AppError::Validation(
                "endpoint_create needs a parent: pass collection_id to put the \
                 endpoint inside a collection, or module_id to put it directly \
                 under a module"
                    .into(),
            )
        })?;
    // 按父节点的真实项目过门。写死字面量 "global" 不是项目 id，过门时只能拿默认的
    // `ReadCall` 去比创建工具要求的 `Full`，于是每次调用都被拒。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &parent_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_create").await?;

    let body_mode_str = a.body_mode.clone().unwrap_or_else(|| "none".into());
    let body = RequestBody {
        mode: parse_body_mode(&body_mode_str)?,
        content: a.body_content.clone().unwrap_or_default(),
        schema: a.body_schema.clone(),
    };
    let auth = build_auth(a.auth_type.as_deref(), a.auth_config.clone())?;

    let now = simple_post_core::domain::now_ms();
    let id = mint_endpoint_id();
    let node = ResourceNode {
        id: id.clone(),
        root_id: project_id,
        parent_id: Some(parent_id),
        kind: ResourceNodeType::Endpoint,
        name: a.name,
        description: a.description,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let detail = EndpointDetail {
        node_id: id,
        method,
        url: a.url,
        environment_id: a.environment_id,
        base_url_definition_id: a.base_url_definition_id,
        base_url_override: a.base_url_override,
        params: coerce_key_values(a.params.unwrap_or_default()),
        headers: coerce_key_values(a.headers.unwrap_or_default()),
        body,
        auth,
        docs: a.docs,
        notes: a.notes,
        tags: a.tags.unwrap_or_default(),
    };
    // node 与 detail 一起落库：没有配置行的端点是一棵打不开的树。
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::create(c, &node, Some(&detail))
    })?;
    let stored = endpoint_resource(&state.pool, &node.id)?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_endpoint_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let endpoint_id = arg_str(&args, "endpoint_id")?.to_string();
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_update").await?;

    let patch_obj = args.get("patch").cloned().unwrap_or(Value::Null);
    if !patch_obj.is_object() {
        return Err(AppError::Validation(format!(
            "endpoint_update expects `patch` to be an object of fields to \
             change, got {patch_obj}"
        )));
    }

    // `name` 是结构、其余是配置，所以一次调用要写两张表。拆分在入口做而不是交给
    // 调用方，`endpoint_update` 才仍然是「改你点名的那些」—— 现有客户端都这么假设。
    //
    // 下面每个字段都用真 `Result`（`patch_field` / `patch_value_field`）读，绝不用
    // `.ok()`。字段有问题会在开事务**之前**中止，于是 patch 是全有或全无：agent 要
    // 么拿到点名了出错字段的 `Validation`，要么点名的字段全写进去。
    let node_patch = simple_post_core::domain::ResourceNodePatch {
        parent_id: None,
        // `name` / `sort_order` 在 patch 类型上是普通 `Option<T>`，没有「清空名字」
        // 这档状态，所以用单层读取器。
        name: patch_value_field::<String>(&patch_obj, "name")?,
        description: patch_field::<String>(&patch_obj, "description")?,
        sort_order: patch_value_field::<i64>(&patch_obj, "sort_order")?,
    };
    let detail_patch = EndpointDetailPatch {
        method: patch_value_field::<simple_post_core::domain::HttpMethod>(&patch_obj, "method")?,
        url: patch_value_field::<String>(&patch_obj, "url")?,
        environment_id: patch_field::<String>(&patch_obj, "environment_id")?,
        base_url_definition_id: patch_field::<String>(&patch_obj, "base_url_definition_id")?,
        base_url_override: patch_field::<String>(&patch_obj, "base_url_override")?,
        params: patch_value_field::<Vec<KeyValueInput>>(&patch_obj, "params")?
            .map(coerce_key_values),
        headers: patch_value_field::<Vec<KeyValueInput>>(&patch_obj, "headers")?
            .map(coerce_key_values),
        body: patch_value_field(&patch_obj, "body")?,
        auth: patch_value_field(&patch_obj, "auth")?,
        // `docs` / `notes` 必须能从这里读进来：它们只能经由本工具写入，写死 `None`
        // 等于让 agent 永远改不了自己刚读到的备注。
        docs: patch_field::<String>(&patch_obj, "docs")?,
        notes: patch_field::<String>(&patch_obj, "notes")?,
        tags: patch_value_field::<Vec<String>>(&patch_obj, "tags")?,
    };
    if node_patch.is_empty() && detail_patch.is_empty() {
        return Err(AppError::Validation(
            "endpoint_update was given an empty patch: name at least one \
             field to change, e.g. {\"patch\":{\"url\":\"/v2/users\"}}. \
             Accepted fields: method, url, params, headers, body, auth, \
             tags, environment_id, base_url_definition_id, \
             base_url_override, docs, notes, name, description, sort_order"
                .into(),
        ));
    }
    // 单事务：改名成功而 url 修改失败，是调用方察觉不到的半截 patch。
    simple_post_core::db::with_tx(&state.pool, |c| {
        if !node_patch.is_empty() {
            simple_post_core::db::nodes::update(c, &endpoint_id, node_patch.clone())?;
        }
        if !detail_patch.is_empty() {
            simple_post_core::db::endpoint_details::update(c, &endpoint_id, detail_patch.clone())?;
        }
        Ok(())
    })?;
    let updated = endpoint_resource(&state.pool, &endpoint_id)?;
    Ok(CallToolResult::json(updated))
}

pub(super) async fn tool_endpoint_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_delete args: {e}")))?;
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::delete(c, &a.endpoint_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.endpoint_id })))
}

pub(super) async fn tool_endpoint_duplicate(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDuplicateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_duplicate args: {e}")))?;
    let ep = endpoint_resource(&state.pool, &a.endpoint_id)?;
    crate::mcp::check_permission(&state.pool, &ep.node.root_id, "endpoint_create").await?;

    // 副本紧挨着原件放，而不是扔到集合末尾 —— 看不出挨着谁的东西很容易被漏掉。
    // 第二个参数给 `None`：MCP 客户端没有乐观行要对齐，副本的名字由服务端起。
    let created = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::duplicate(c, &a.endpoint_id, None)
    })?;
    let created = match a.name {
        Some(name) => simple_post_core::db::with_tx(&state.pool, |c| {
            simple_post_core::db::nodes::update(
                c,
                &created.id,
                simple_post_core::domain::ResourceNodePatch {
                    name: Some(name.clone()),
                    ..Default::default()
                },
            )
        })?,
        None => created,
    };
    let stored = endpoint_resource(&state.pool, &created.id)?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_endpoint_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_move args: {e}")))?;
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_update").await?;

    // 单事务、单行、id 保持不变。
    //
    // 「删掉重建」既会丢数据（中途崩溃 = 端点没了），也会让调用方的 `endpoint_id`
    // 悬空 —— 响应还得告诉它新 id 是什么。`nodes::move_many` 是原地换父级。
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::move_many(
            c,
            std::slice::from_ref(&a.endpoint_id),
            Some(&a.new_collection_id),
            a.new_sort_order.unwrap_or(0) as i32,
        )
    })?;
    Ok(CallToolResult::json(json!({
        "endpoint_id": a.endpoint_id,
        "new_collection_id": a.new_collection_id,
    })))
}

// =========================================================================
// 私有辅助函数（只被上面的 endpoint_* 工具使用）
// =========================================================================

fn parse_body_mode(s: &str) -> Result<simple_post_core::domain::BodyMode, AppError> {
    use simple_post_core::domain::BodyMode;
    Ok(match s.to_ascii_lowercase().as_str() {
        "none" => BodyMode::None,
        "json" => BodyMode::Json,
        "form" => BodyMode::Form,
        "raw" => BodyMode::Raw,
        other => {
            return Err(AppError::Validation(format!(
                "unknown body_mode {other:?}; expected one of: {}",
                BODY_MODE_VALUES.join(", ")
            )))
        }
    })
}

/// 线上接受的 `body_mode` 取值，与 [`simple_post_core::domain::BodyMode`] 的变体
/// 一一对应；JSON Schema 与解析器共用这一份，改一处即可。
const BODY_MODE_VALUES: &[&str] = &["none", "json", "form", "raw"];

/// 线上接受的 `auth_type` 取值，对应 [`simple_post_core::domain::AuthType`] 的小写
/// 变体名。
const AUTH_TYPE_VALUES: &[&str] = &["none", "bearer", "basic", "apikey"];

fn build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<simple_post_core::domain::AuthConfig, AppError> {
    use simple_post_core::domain::{AuthConfig, AuthType};
    let kind = match auth_type.unwrap_or("none").to_ascii_lowercase().as_str() {
        "none" => AuthType::None,
        "bearer" => AuthType::Bearer,
        "basic" => AuthType::Basic,
        "apikey" => AuthType::Apikey,
        other => {
            return Err(AppError::Validation(format!(
                "unknown auth_type {other:?}; expected one of: {}",
                AUTH_TYPE_VALUES.join(", ")
            )))
        }
    };
    let cfg = auth_config.unwrap_or(Value::Null);
    // `auth_config` 用真 `Result` 反序列化，绝不 `unwrap_or_default()`：配置写错时
    // 静默退化成全空 `AuthConfig`，agent 会拿到一个 SUCCESS 响应，里面描述的却是
    // 根本没有 token 的 bearer。
    //
    // `kind` 经 `AuthConfigWire` 而不是直接进 `AuthConfig`：域类型在 `kind` 上没有
    // serde 默认值，于是 `{"bearer":"tok"}` —— JSON Schema 文档化的、agent 实际会
    // 写的那种形状 —— 会因缺字段而失败。方案真正是在 `auth_type` 里选的，下面会
    // 写回结果，所以 config 里的 `kind` 是冗余而非必需。
    let mut out: AuthConfig = match cfg {
        Value::Null => AuthConfig::default(),
        other => {
            let wire: AuthConfigWire = serde_json::from_value(other).map_err(|e| {
                AppError::Validation(format!(
                    "auth_config is malformed ({e}); expected an object like \
                     {{\"bearer\":\"<token>\"}} for bearer, \
                     {{\"basic\":{{\"username\":\"u\",\"password\":\"p\"}}}} for basic, \
                     {{\"apikey\":{{\"key\":\"X-Api-Key\",\"value\":\"<key>\",\
                     \"inLocation\":\"header\"}}}} for apikey"
                ))
            })?;
            wire.into_auth_config()
        }
    };
    out.kind = kind;
    Ok(out)
}

/// `kind` 可选的 `AuthConfig` —— 理由见 [`build_auth`]。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthConfigWire {
    #[serde(default)]
    kind: Option<simple_post_core::domain::AuthType>,
    #[serde(default)]
    bearer: Option<String>,
    #[serde(default)]
    basic: Option<simple_post_core::domain::BasicAuth>,
    #[serde(default)]
    apikey: Option<simple_post_core::domain::ApiKeyAuth>,
}

impl AuthConfigWire {
    fn into_auth_config(self) -> simple_post_core::domain::AuthConfig {
        simple_post_core::domain::AuthConfig {
            // 由调用方按 `auth_type` 覆写；保留它只为让「完全不带 `auth_type`、
            // 但 config 自带 `kind`」的形状能原样往返。
            kind: self.kind.unwrap_or_default(),
            bearer: self.bearer,
            basic: self.basic,
            apikey: self.apikey,
        }
    }
}

// =========================================================================
// 严格的 patch 字段读取器
// =========================================================================
//
// 每个读取器都保持的契约：
//
//   * 键缺失          → `None` → 该列不动
//   * 显式 JSON `null` → `Some(None)` → 该列清空（仅 `double_option` 字段）
//   * 有值且格式正确   → `Some(Some(v))` → 写入
//   * 有值但格式错误   → `Err(AppError::Validation)`，点名字段与期望形状
//
// 关键是第四行。`.ok()` 会把「字段缺失」与「字段格式错」压成同一个结果，于是一个
// 写坏的 patch 会返回 `is_error: false` 却什么都没写 —— agent 无法区分「改成功」与
// 「被丢掉」。
//
// 每一行的 `?` 都意味着格式错误会在开事务**之前**中止整个调用，于是 patch 绝不会
// 只应用一半：要么点名的字段全写进去，要么一个不写。

/// 附在「字段格式错」错误后的可读形状提示，逐字段写明 —— agent 下一步就是重发
/// patch，不该再去猜哪个嵌套键写错了。
fn patch_shape_hint(field: &str) -> String {
    match field {
        "method" => "a string HTTP verb, e.g. \"GET\", \"POST\", \"PUT\", \
                     \"DELETE\", \"PATCH\", \"HEAD\", \"OPTIONS\" (or any RFC \
                     9110 token such as \"PROPFIND\")"
            .to_string(),
        "url" => "a string, e.g. \"/api/users/{id}\"".to_string(),
        "params" | "headers" => "an array of objects shaped \
             {\"key\":\"<name>\",\"value\":\"<value>\",\"enabled\":true} \
             — note the field names are `key` and `value`, not `k`/`v`/`name`"
            .to_string(),
        "body" => format!(
            "an object shaped {{\"mode\":\"<{}>\",\"content\":\"<raw body>\",\
             \"schema\":\"<optional>\"}}",
            BODY_MODE_VALUES.join("|")
        ),
        "auth" => format!(
            "an object shaped {{\"kind\":\"<{}>\", ...}} where the rest \
             depends on kind: bearer → {{\"bearer\":\"<token>\"}}, basic → \
             {{\"basic\":{{\"username\":\"u\",\"password\":\"p\"}}}}, apikey → \
             {{\"apikey\":{{\"key\":\"<header name>\",\"value\":\"<key>\",\
             \"inLocation\":\"header\"|\"query\"}}}}",
            AUTH_TYPE_VALUES.join("|")
        ),
        "tags" => "an array of strings, e.g. [\"smoke\",\"user\"]".to_string(),
        "environment_id"
        | "base_url_definition_id"
        | "base_url_override"
        | "description"
        | "docs"
        | "notes" => "a string, or null to clear the field".to_string(),
        "name" => "a non-empty string".to_string(),
        "sort_order" => "an integer".to_string(),
        _ => String::new(),
    }
}

/// 把某个字段的反序列化失败包成 agent 看得懂的 `Validation` 错误。
fn malformed_field(field: &str, detail: impl std::fmt::Display) -> AppError {
    let hint = patch_shape_hint(field);
    if hint.is_empty() {
        AppError::Validation(format!("patch field '{field}' is malformed: {detail}"))
    } else {
        AppError::Validation(format!(
            "patch field '{field}' is malformed: {detail}. Expected {hint}"
        ))
    }
}

/// 读一个 patch 字段，保持 `double_option` 的三档语义：缺失 → `None`（不动）、
/// `null` → `Some(None)`（清空）、有值 → `Some(Some(v))`。
///
/// 反序列化失败是**错误**，不是静默的「别动」。
fn patch_field<T: serde::de::DeserializeOwned>(
    patch_obj: &Value,
    field: &str,
) -> Result<Option<Option<T>>, AppError> {
    let Some(raw) = patch_obj.get(field) else {
        return Ok(None);
    };
    if raw.is_null() {
        // `double_option` 语义：显式 null 是「清空这一列」，一个真实的写入目标，
        // 不是「别动」。
        return Ok(Some(None));
    }
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(v) => Ok(Some(Some(v))),
        Err(e) => Err(malformed_field(field, e)),
    }
}

/// 读一个没有「用 null 清空」语义的 patch 字段 —— 值本身就是载荷（`params`、
/// `headers`、`tags`、`body`、`auth`）。
///
/// 这里的 `null` 是**笔误**而非清空请求：这些列要靠发空数组 / 空 body 才清得掉；
/// 把 `null` 当清空，会让「把缺失值序列化成 null」的客户端悄悄抹掉配好的请求头列表。
fn patch_value_field<T: serde::de::DeserializeOwned>(
    patch_obj: &Value,
    field: &str,
) -> Result<Option<T>, AppError> {
    let Some(raw) = patch_obj.get(field) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Err(malformed_field(field, "got null"));
    }
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(v) => Ok(Some(v)),
        Err(e) => Err(malformed_field(field, e)),
    }
}

/// agent 手写的一行 `KeyValue` 的线上形状。
///
/// [`simple_post_core::domain::KeyValue`] 的 `id` 与 `enabled` 没有 serde 默认值，
/// 于是最自然的写法 `{"key":"page","value":"1"}` 反序列化失败。两处放宽的理由不同：
///
/// * `id` 可选，缺失时另铸。纯簿记字段 —— UI 拿它在 React 里做行 key，请求组装不读。
///   客户端真传了就保留，于是把上一行原样回传的编辑是就地更新，不会把它变成孤儿。
/// * `enabled` 可选，且**默认 `true`**，不是 `KeyValue` 自己的 `Default`（`false`）。
///   存成禁用的请求头就是一个永远发不出去的请求头；agent 点名一个请求头就是要发它。
///   显式 `false` 仍然照办，那对应「留着但先关掉」。
///
/// `value` 用同样方式放宽，因为「故意发一个空请求头值」是真事，不是笔误。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KeyValueInput {
    #[serde(default)]
    id: Option<String>,
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    description: Option<String>,
}

impl KeyValueInput {
    fn into_key_value(self) -> simple_post_core::domain::KeyValue {
        let id = match self.id {
            Some(id) if !id.trim().is_empty() => id,
            _ => simple_post_core::domain::uid(),
        };
        simple_post_core::domain::KeyValue {
            id,
            key: self.key,
            value: self.value,
            enabled: self.enabled.unwrap_or(true),
            description: self.description,
        }
    }
}

/// 反序列化 `params` / `headers` 数组，并补齐 `KeyValue` 要求、而 agent 没有理由
/// 知道的那些簿记字段。
fn coerce_key_values(rows: Vec<KeyValueInput>) -> Vec<simple_post_core::domain::KeyValue> {
    rows.into_iter()
        .map(KeyValueInput::into_key_value)
        .collect()
}

// =========================================================================
// 仅测试用的再导出
// =========================================================================
//
// `mod.rs::tests` 需要直接调 `parse_body_mode` 与 `build_auth`。它们是上面这些文件私
// 有的辅助函数，用 `#[cfg(test)]` 暴露，生产调用方看不到。

#[cfg(test)]
pub(super) fn __test_parse_body_mode(
    s: &str,
) -> Result<simple_post_core::domain::BodyMode, AppError> {
    parse_body_mode(s)
}

#[cfg(test)]
pub(super) fn __test_build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<simple_post_core::domain::AuthConfig, AppError> {
    build_auth(auth_type, auth_config)
}

// =========================================================================
// 测试
// =========================================================================
//
// 兄弟 `#[path]` 模块，遵循本 crate 的惯例：Rust 按模块路径找文件，所以测试专用
// *目录* 是找不到的；内联 `mod tests` 在这里也是少数派。

#[cfg(test)]
#[path = "endpoint_test.rs"]
mod tests;
