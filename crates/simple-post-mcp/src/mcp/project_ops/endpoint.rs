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
    pub collection_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Option<Vec<simple_post_core::domain::KeyValue>>,
    #[serde(default)]
    pub params: Option<Vec<simple_post_core::domain::KeyValue>>,
    #[serde(default)]
    pub body_mode: Option<String>,
    #[serde(default)]
    pub body_content: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
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
    #[serde(default)]
    #[allow(dead_code)] // accepted by JSON Schema; filtering happens at the SQL layer
    pub module_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // accepted by JSON Schema; filtering happens at the SQL layer
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
        description: "List endpoints in a project / module / collection.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "module_id": { "type": "string" },
                "collection_id": { "type": "string" }
            },
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
        description: "Search endpoints by free-text query, method, or tags.",
        schema: json!({
            "type": "object",
            "properties": {
                "project_id": { "type": "string" },
                "module_id": { "type": "string" },
                "collection_id": { "type": "string" },
                "q": { "type": "string" },
                "method": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_endpoint_search),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_create",
        description: "Create a new endpoint inside a collection.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "name": { "type": "string" },
                "method": { "type": "string" },
                "url": { "type": "string" },
                "headers": { "type": "array", "items": { "type": "object" } },
                "params": { "type": "array", "items": { "type": "object" } },
                "body_mode": { "type": "string" },
                "body_content": { "type": "string" },
                "auth_type": { "type": "string" },
                "auth_config": { "type": "object" },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["collection_id", "name", "method", "url"],
            "additionalProperties": true,
        }),
        func: tool_fn!(tool_endpoint_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "endpoint_update",
        description: "Patch an endpoint's fields.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "patch": { "type": "object" }
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
        description: "Move an endpoint to a new collection / position.",
        schema: json!({
            "type": "object",
            "properties": {
                "endpoint_id": { "type": "string" },
                "new_collection_id": { "type": "string" },
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
    let _ = arg_str_opt(&args, "module_id"); // accepted but not used in this stub path

    crate::mcp::check_permission(
        &state.pool,
        project_id.as_deref().unwrap_or("global"),
        "endpoint_list",
    )
    .await?;

    // `module_id` narrows to a module's whole subtree, because an
    // endpoint's ancestor chain is now uniform — every node except a
    // module has a node parent.
    let scope = collection_id
        .as_deref()
        .map(|c| ("collection", c.to_string()))
        .or_else(|| arg_str_opt(&args, "module_id").map(|m| ("module", m.to_string())))
        .or_else(|| project_id.clone().map(|p| ("project", p)));

    let rows = match scope {
        Some(("collection", cid)) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_children(c, &cid, Some(ResourceNodeType::Endpoint))
        })?,
        Some(("module", mid)) => {
            let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&mid))
            })?;
            let project = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::root_id_of(c, &mid)
            })?;
            let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
            let nodes = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::list_by_root_and_type(
                    c,
                    &project,
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
        // Nothing named: the whole database, as the old unfiltered
        // `endpoints::search` did.
        None => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_all(c)
        })?,
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
    crate::mcp::check_permission(
        &state.pool,
        a.project_id.as_deref().unwrap_or("global"),
        "endpoint_list",
    )
    .await?;
    // `method` and `tags` live in `endpoint_details`, so the free-text
    // match and those two filters are applied after narrowing to
    // endpoint nodes — the SQL layer matches name / url / description
    // only.
    let mut nodes = match a.q.as_deref().filter(|q| !q.is_empty()) {
        Some(q) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::search(c, q)
        })?,
        None => match a.project_id.as_deref() {
            Some(p) => simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::list_by_root_and_type(c, p, ResourceNodeType::Endpoint)
            })?,
            None => simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::list_all(c)
            })?,
        },
    };
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
    // Gate against the collection's real project. The old handler passed
    // the literal `"global"`, which is not a project id — `check_permission`
    // then compared the default `ReadCall` against the `Full` a creating
    // tool requires and denied every call.
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &a.collection_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_create").await?;

    let body_mode_str = a.body_mode.clone().unwrap_or_else(|| "none".into());
    let body = RequestBody {
        mode: parse_body_mode(&body_mode_str)?,
        content: a.body_content.clone().unwrap_or_default(),
        schema: None,
    };
    let auth = build_auth(a.auth_type.as_deref(), a.auth_config.clone())?;

    let now = simple_post_core::domain::now_ms();
    let id = format!("ep_{now}");
    let node = ResourceNode {
        id: id.clone(),
        root_id: project_id,
        parent_id: Some(a.collection_id),
        kind: ResourceNodeType::Endpoint,
        name: a.name,
        description: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let detail = EndpointDetail {
        node_id: id,
        method,
        url: a.url,
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: a.params.unwrap_or_default(),
        headers: a.headers.unwrap_or_default(),
        body,
        auth,
        docs: None,
        notes: None,
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

    // `name` is structural and everything else is configuration, so one
    // call now writes two tables. The patch is split on the way in
    // rather than by the caller so `endpoint_update` keeps meaning
    // "patch whatever you named" — which is what every existing client
    // assumes.
    let node_patch = simple_post_core::domain::ResourceNodePatch {
        parent_id: None,
        name: patch_obj
            .get("name")
            .and_then(Value::as_str)
            .map(String::from),
        description: patch_obj
            .get("description")
            .map(|v| v.as_str().map(String::from)),
        sort_order: patch_obj.get("sort_order").and_then(Value::as_i64),
    };
    let detail_patch = EndpointDetailPatch {
        method: patch_obj
            .get("method")
            .and_then(Value::as_str)
            .and_then(|m| simple_post_core::domain::parse_http_method(m).ok()),
        url: patch_obj
            .get("url")
            .and_then(Value::as_str)
            .map(String::from),
        environment_id: patch_obj
            .get("environment_id")
            .map(|v| v.as_str().map(String::from)),
        base_url_definition_id: patch_obj
            .get("base_url_definition_id")
            .map(|v| v.as_str().map(String::from)),
        base_url_override: patch_obj
            .get("base_url_override")
            .map(|v| v.as_str().map(String::from)),
        params: patch_obj
            .get("params")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        headers: patch_obj
            .get("headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        body: patch_obj
            .get("body")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        auth: patch_obj
            .get("auth")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        docs: None,
        notes: None,
        tags: patch_obj.get("tags").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        }),
    };
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
    // duplicated is easy to miss.
    let created = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::duplicate(c, &a.endpoint_id)
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
        other => return Err(AppError::Validation(format!("unknown body_mode: {other}"))),
    })
}

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
        other => return Err(AppError::Validation(format!("unknown auth type: {other}"))),
    };
    let cfg = auth_config.unwrap_or(Value::Null);
    // Best-effort deserialize of auth_config. The TS side uses `{}` when
    // there's nothing to set, which is fine for any auth type.
    let mut out: AuthConfig = serde_json::from_value(cfg).unwrap_or_default();
    out.kind = kind;
    Ok(out)
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
