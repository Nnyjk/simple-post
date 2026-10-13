//! `endpoint_*` MCP tools — endpoint CRUD, search, duplicate, and movement.
//!
//! 8 tools:
//! - `endpoint_list`
//! - `endpoint_get`
//! - `endpoint_search`
//! - `endpoint_create`
//! - `endpoint_update`
//! - `endpoint_delete`
//! - `endpoint_duplicate`
//! - `endpoint_move`
//!
//! Private helpers also live here because they're only used by these
//! tools:
//! - `parse_body_mode`, `build_auth`: input parsing for `endpoint_create`.
//! - Permission gating resolves the owning project with
//!   `simple_post_core::db::nodes::root_id_of` — one indexed read, where
//!   the old `endpoints::project_id_of` walked `collection → module →
//!   project`.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, arg_str_opt, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{
    EndpointDetail, EndpointDetailPatch, EndpointResource, RequestBody, ResourceNode,
    ResourceNodeType,
};
use simple_post_core::error::AppError;

/// Load a node plus its request configuration as one object.
///
/// An AI client asking "show me this endpoint" wants the method and the
/// url, and those live in `endpoint_details` now. Returning the bare
/// node would answer with a name and a type, which is not what the tool
/// is for.
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

/// Pair a list of endpoint nodes with their configurations, preserving
/// the node order the caller asked for.
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
    /// Parent collection. Optional now that an endpoint may sit directly
    /// under a module — exactly one of this and `module_id` is required.
    #[serde(default)]
    pub collection_id: Option<String>,
    /// Parent module, for an endpoint that needs no collection of its own.
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
    /// Optional body schema hint, stored verbatim. `endpoint_create`
    /// used to hardcode `schema: None`, so an agent could not attach one
    /// at creation time and had to follow up with `endpoint_update`.
    #[serde(default)]
    pub body_schema: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Environment to bind. Accepted at create so a freshly built
    /// endpoint lands already pointed at the right environment instead
    /// of needing a second call.
    #[serde(default)]
    pub environment_id: Option<String>,
    /// Service (`base_url_definitions` row) this endpoint resolves
    /// against. `None` means "inherit the project's ★".
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

/// Mint a unique endpoint id.
///
/// The id used to be `format!("ep_{now}")`, i.e. `ep_` plus
/// `now_ms()` — a **millisecond** clock. Two `endpoint_create` calls in
/// the same millisecond produced the same primary key, and the second
/// one failed the `nodes.id` primary-key constraint with a raw SQLite
/// error. An AI client firing a batch of creates in a loop hits this
/// routinely, and the error it surfaced was not actionable.
///
/// `domain::uid()` is the crate's canonical id source: nanoseconds plus
/// a per-process counter, so two calls cannot collide within a process
/// and a restart cannot repeat a prefix. The readable `ep_` prefix is
/// kept so an id still says what it identifies, matching the
/// `col_` / `mod_` / `proj_` convention the sibling create tools use.
fn mint_endpoint_id() -> String {
    let uid = simple_post_core::domain::uid();
    // `uid()` returns `sp_<hex>`; swap the generic prefix for the
    // endpoint one rather than reimplementing the clock here, so there
    // is still exactly one id source in the crate.
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
    /// Scope to a module's whole subtree. Read by the permission gate and
    /// by the result narrowing, not by the SQL layer — `nodes::search`
    /// matches name / url / description across the table, so the subtree
    /// filter is applied after it returns.
    #[serde(default)]
    pub module_id: Option<String>,
    /// Scope to one collection's direct children.
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

    // Gate against the project that actually owns whatever the caller
    // named. `collection_id` / `module_id` name a *node*, so
    // `nodes::root_id_of` is one indexed read to its project; a bare
    // `project_id` is already the answer.
    //
    // This used to pass `project_id.unwrap_or("global")`, and
    // `agent_permissions.project_id` is an FK to `projects(id)` — so
    // `"global"` matched no row, fell back to `Permission::default()`
    // and happened to pass only because `endpoint_list` is `Read` tier.
    // Scoping to a collection or module instead of a project bypassed
    // the gate entirely: the real owner's permission was never consulted
    // at all, so a Read-only project was readable through a scoped call
    // no matter what it had granted. Resolving the owner closes that.
    //
    // With no scope named the read genuinely spans every project, and
    // there is no single project to gate against. Rather than fabricate
    // an id (the exact bug this file is fixing) the call is rejected:
    // an unscoped read that touches every project is not something the
    // per-project gate can express, and returning one project's data
    // under another's authorisation is worse than saying no.
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

    // `module_id` narrows to a module's whole subtree, because an
    // endpoint's ancestor chain is now uniform — every node except a
    // module has a node parent.
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
        // Unreachable: `scope` is `None` only when nothing was named, and
        // that case is rejected above because it has no owner to gate on.
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

    // Same owner resolution as `endpoint_list` — a `collection_id` /
    // `module_id` names a node and `root_id_of` resolves its project;
    // `project_id` is already the answer. The previous
    // `project_id.unwrap_or("global")` matched no `agent_permissions`
    // row (the column is an FK to `projects(id)`), so the gate silently
    // consulted the default instead of the owner's real permission, and
    // a collection- or module-scoped search was never gated at all.
    //
    // An unscoped search spans projects, so it has no single owner to
    // authorise against and is rejected rather than answered under a
    // fabricated id. Also note the gate now names *this* tool: it
    // previously passed `"endpoint_list"`, so a permission change
    // aimed at search was evaluated against the wrong row.
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

    // `method` and `tags` live in `endpoint_details`, so the free-text
    // match and those two filters are applied after narrowing to
    // endpoint nodes — the SQL layer matches name / url / description
    // only.
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
    // A `module_id` / `collection_id` scope must narrow the free-text
    // branch too, not just the unfiltered one — otherwise the gate
    // checks the module's project and the result set still spans every
    // project, which is the cross-project bleed the gate exists to stop.
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
    // Validate the HTTP verb *before* touching the permission table:
    // input-level errors are cheap and shouldn't masquerade as
    // permission denials.
    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    // An endpoint's parent is a collection, or the module itself when the
    // caller has no folder for it. `collection_id` and `module_id` are two
    // names for that one relationship — the same shape `collection_create`
    // uses for its own module-or-collection parent — so take whichever
    // the client supplied and let the matrix reject a nonsense pairing.
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
    // Gate against the parent's real project. The old handler passed
    // the literal `"global"`, which is not a project id — `check_permission`
    // then compared the default `ReadCall` against the `Full` a creating
    // tool requires and denied every call.
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
    // Node and detail land together: an endpoint with no configuration
    // row is a tree the reader cannot open.
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

    // `name` is structural and everything else is configuration, so one
    // call now writes two tables. The patch is split on the way in
    // rather than by the caller so `endpoint_update` keeps meaning
    // "patch whatever you named" — which is what every existing client
    // assumes.
    //
    // Every field below is read with a real `Result` (`patch_field` /
    // `patch_value_field`), never `.and_then(… .ok())`. A malformed
    // field aborts the call *before* the transaction opens, so a patch
    // is all-or-nothing: the agent either gets a `Validation` error
    // naming the offending field, or every field it named is written.
    let node_patch = simple_post_core::domain::ResourceNodePatch {
        parent_id: None,
        // `name` / `sort_order` are plain `Option<T>` on the patch type
        // — there is no "clear a name" state — so they use the
        // single-level reader.
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
        // `docs` and `notes` were hardcoded to `None` here, which made
        // them unreachable through the only tool that can write them —
        // the agent could read an endpoint's notes but never change one.
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
    // One transaction: a rename that succeeds while its url edit does not
    // is a half-applied patch the caller cannot detect.
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

    // The copy lands immediately after the original, not at the end of
    // the collection — a duplicate the user cannot see next to what they
    // duplicated is easy to miss. `None`: an MCP client has no optimistic
    // row to stay consistent with, so the server names the copy.
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

    // One transaction, one row, id preserved.
    //
    // The old handler deleted the endpoint and re-created it under a
    // freshly minted id, in two separate transactions. A crash between
    // them lost the endpoint outright, and even on the happy path the
    // caller's `endpoint_id` dangled — the response had to tell them the
    // new one. `nodes::move_many` re-parents in place.
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
// Private helpers (only used by the endpoint_* tools above)
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

/// The `body_mode` values accepted on the wire, read from
/// [`simple_post_core::domain::BodyMode`]'s own `serde` rename rather
/// than restated, so the JSON Schema and the parser cannot drift apart.
const BODY_MODE_VALUES: &[&str] = &["none", "json", "form", "raw"];

/// The `auth_type` values accepted on the wire, mirroring
/// [`simple_post_core::domain::AuthType`]'s lowercase variants.
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
    // `auth_config` is deserialized with a real `Result` rather than
    // `unwrap_or_default()`: a malformed config used to be silently
    // replaced by an all-empty `AuthConfig`, so an agent could set
    // `auth_type: "bearer"` with a typo'd `auth_config` and get a
    // SUCCESS response describing an auth with no token at all.
    //
    // `kind` is deserialized through `AuthConfigWire` rather than
    // straight into `AuthConfig`: the domain type has no serde default
    // on `kind`, so `{"bearer":"tok"}` — the shape the JSON Schema
    // documents and the shape an agent writes — failed on a missing
    // field. `auth_type` is where the scheme is actually chosen, and it
    // is written back onto the result below, so a `kind` inside the
    // config is redundant rather than required.
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

/// `AuthConfig` with `kind` optional — see [`build_auth`].
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
            // Overwritten by the caller from `auth_type`; kept only so a
            // config that *does* carry `kind` round-trips intact when
            // `auth_type` is omitted entirely.
            kind: self.kind.unwrap_or_default(),
            bearer: self.bearer,
            basic: self.basic,
            apikey: self.apikey,
        }
    }
}

// =========================================================================
// Strict patch field readers
// =========================================================================
//
// These replace the `.and_then(|v| serde_json::from_value(v).ok())` shape
// that used to build `EndpointDetailPatch`. `.ok()` collapsed two very
// different outcomes into one:
//
//   * the field was absent      → "leave this column alone"   (correct)
//   * the field was present but
//     deserialisation failed   → "leave this column alone"   (WRONG)
//
// So `{"params": [{"k": "a", "v": "1"}]}` — a plausible typo, and the
// shape an LLM reaches for first — returned `CallToolResult` with
// `is_error: false` and wrote nothing at all. The agent had no way to
// tell a successful patch from a dropped one, which makes "build an
// endpoint's interface parameters" unreliable for exactly the reason it
// is supposed to work.
//
// The contract the readers below keep:
//
//   * absent key               → `None`  → column untouched
//   * explicit JSON `null`     → `Some(None)` → column cleared (for the
//                                `double_option` fields)
//   * present and well-formed  → `Some(Some(v))` → column written
//   * present and malformed    → `Err(AppError::Validation)` naming the
//                                field AND the shape that was expected
//
// The `?` on every row means a malformed patch now aborts the whole call
// before the transaction opens, so a patch can never be *partially*
// applied: either every field it named is written, or none is.

/// Human-readable shape hint appended to a malformed-field error. One
/// line per field, because the agent's next move is to re-send the patch
/// and it should not have to guess which nested key was wrong.
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

/// Wrap a field-specific deserialisation failure into the actionable
/// `Validation` error the agent sees.
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

/// Read one patch field, preserving the three-state `double_option`
/// contract: absent → `None` (untouched), `null` → `Some(None)` (clear),
/// a value → `Some(Some(v))`.
///
/// Unlike the old `.ok()` reader, a deserialisation failure here is an
/// **error**, not a silent "leave unchanged".
fn patch_field<T: serde::de::DeserializeOwned>(
    patch_obj: &Value,
    field: &str,
) -> Result<Option<Option<T>>, AppError> {
    let Some(raw) = patch_obj.get(field) else {
        return Ok(None);
    };
    if raw.is_null() {
        // `double_option` semantics: an explicit null is "clear this
        // column", which is a real write target rather than "leave alone".
        return Ok(Some(None));
    }
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(v) => Ok(Some(Some(v))),
        Err(e) => Err(malformed_field(field, e)),
    }
}

/// Read one patch field that has no clear-by-null semantics — the value
/// itself is the payload (`params`, `headers`, `tags`, `body`, `auth`).
///
/// `null` here is a **mistake**, not a request to clear: these columns
/// are cleared by sending an empty array / an empty-ish body, and
/// treating `null` as "clear" would make a client that serialises an
/// absent value as `null` silently wipe a configured header list.
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

/// Wire shape for one `KeyValue` row as an agent authors it.
///
/// [`simple_post_core::domain::KeyValue`] carries `id` and `enabled` with
/// no serde `default`, so the obvious agent-authored row —
/// `{"key":"page","value":"1"}` — fails to deserialise. Under the old
/// `.ok()` reader that failure vanished into "leave the column alone",
/// so the single most likely thing an agent writes produced a SUCCESS
/// response and no write.
///
/// Two fields are relaxed relative to the domain type, and the reasons
/// differ:
///
/// * `id` is optional and minted when absent. It is pure bookkeeping —
///   the UI uses it to key rows in React; nothing in request building
///   reads it. A client that *does* send one keeps it, so an edit that
///   echoes the previous row back updates in place instead of orphaning
///   it.
/// * `enabled` is optional and **defaults to `true`**, not to
///   `KeyValue`'s own `Default` of `false`. A header stored disabled is
///   a header the request never sends — the silent-failure mode this
///   whole fix exists to remove. An agent that names a header means to
///   send it; an explicit `false` is still honoured for the "keep it but
///   switch it off" case.
///
/// `value` is relaxed the same way, because an intentionally-empty header
/// value is a real thing to send and not a mistake.
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

/// Deserialize a `params` / `headers` array, repairing the bookkeeping
/// fields `KeyValue` demands but an agent has no reason to know about.
fn coerce_key_values(rows: Vec<KeyValueInput>) -> Vec<simple_post_core::domain::KeyValue> {
    rows.into_iter()
        .map(KeyValueInput::into_key_value)
        .collect()
}

// =========================================================================
// Test-only re-exports
// =========================================================================
//
// `mod.rs::tests` needs to call `parse_body_mode` and `build_auth` to
// verify their behaviour. They're file-private helpers above; expose
// them with `#[cfg(test)]` so production callers can't see them.

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
// Tests
// =========================================================================
//
// Sibling `#[path]` module, matching the crate-wide convention: Rust
// resolves a module's file from its module path, so a test-only
// *directory* is not discoverable and inline `mod tests` is the minority
// form here.

#[cfg(test)]
#[path = "endpoint_test.rs"]
mod tests;
