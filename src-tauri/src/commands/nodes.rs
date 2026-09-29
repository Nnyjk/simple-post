// ===========================================================================
// region: nodes
// ===========================================================================
//
// Replaces `modules_*`, `collections_*` and `endpoints_*` — nineteen Tauri
// commands that existed only because the tree was three tables. The
// handlers below are the Tauri half of the same surface `web_api/routes/
// nodes.rs` serves over REST; both go through `crate::db::nodes` so the
// two transports cannot diverge.
//
// Two commands are not about structure and keep their own handlers:
// `nodes_duplicate` and the endpoint-detail pair.

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::CreateEndpointDetailOpts;

/// Every node under one project, or the whole database when no root is
/// named.
///
/// `kind` narrows to a single type and `query` runs the substring search
/// (which joins `endpoint_details`, since `url` lives there). `query`
/// wins over `kind` — a search across one type is a filter the caller
/// can apply, and a backend that guessed between them would be guessing.
#[tauri::command]
pub fn nodes_list(
    state: State<DbPool>,
    root_id: Option<String>,
    kind: Option<String>,
    query: Option<String>,
) -> Result<Vec<ResourceNode>, AppError> {
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        return crate::db::with_conn(&state, |c| crate::db::nodes::search(c, &q));
    }
    let kind = match kind.filter(|k| !k.is_empty()) {
        Some(k) => Some(
            ResourceNodeType::parse(&k)
                .ok_or_else(|| AppError::Validation(format!("unknown node type {k:?}")))?,
        ),
        None => None,
    };
    // A type filter without a root would scan the whole database and
    // throw most of it away. Say so rather than quietly returning the
    // tree for a different project than the caller has active.
    if kind.is_some() && root_id.is_none() {
        return Err(AppError::Validation("type filter requires rootId".into()));
    }
    crate::db::with_conn(&state, |c| match (kind, root_id.as_deref()) {
        (Some(k), Some(root)) => crate::db::nodes::list_by_root_and_type(c, root, k),
        (None, Some(root)) => crate::db::nodes::list_by_root(c, root),
        (None, None) => crate::db::nodes::list_all(c),
        // Unreachable: guarded above.
        (Some(_), None) => Ok(Vec::new()),
    })
}

#[tauri::command]
pub fn nodes_get(state: State<DbPool>, id: String) -> Result<ResourceNode, AppError> {
    crate::db::with_conn(&state, |c| crate::db::nodes::get(c, &id))
}

/// Create a node. An endpoint arrives as one call carrying both halves —
/// the structural node and its request configuration — because they are
/// written in a single transaction and a tree where either can be missing
/// is a tree the reader cannot open.
///
/// `root_id` is only required for a module. For a collection or an
/// endpoint it is derived from the parent, so the two can never disagree.
#[tauri::command]
pub fn nodes_create(
    state: State<DbPool>,
    root_id: Option<String>,
    parent_id: Option<String>,
    kind: String,
    name: String,
    description: Option<String>,
    detail: Option<CreateEndpointDetailOpts>,
) -> Result<ResourceNode, AppError> {
    let kind = ResourceNodeType::parse(&kind)
        .ok_or_else(|| AppError::Validation(format!("unknown node type {kind:?}")))?;
    let now = crate::domain::now_ms();
    let mut node = ResourceNode {
        id: uid(),
        root_id: root_id.unwrap_or_default(),
        parent_id,
        kind,
        name,
        description,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    if node.root_id.is_empty() {
        if let Some(parent) = node.parent_id.clone() {
            node.root_id =
                crate::db::with_conn(&state, |c| crate::db::nodes::root_id_of(c, &parent))?;
        }
    }
    let detail = detail.map(|d| d.into_detail(&node.id));
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::create(c, &node, detail.as_ref())
    })
}

/// Rename / re-describe / re-order one node. Moving is a separate command
/// — see [`nodes_move`] — because it has to keep the type matrix and the
/// subtree's `root_id` honest.
#[tauri::command]
pub fn nodes_update(
    state: State<DbPool>,
    id: String,
    patch: ResourceNodePatch,
) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::update(c, &id, patch.clone()))
}

/// Delete a node and its whole subtree.
#[tauri::command]
pub fn nodes_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::delete(c, &id))
}

/// Move one or many nodes under a new parent at a new sibling position.
///
/// `parent_id` of `None` means "top of the project" and is only legal for
/// a module — the old `parentKind` field is gone because the destination
/// parent's own type answers the question. `node_ids` is in the order the
/// nodes should land, so a multi-select drag keeps the user's pick order.
#[tauri::command]
pub fn nodes_move(
    state: State<DbPool>,
    node_ids: Vec<String>,
    parent_id: Option<String>,
    index: i32,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::move_many(c, &node_ids, parent_id.as_deref(), index)
    })
}

/// Copy an endpoint under the same parent, immediately after it.
///
/// Endpoints only: a "copy" of a module or collection would be an empty
/// shell, and copying a subtree is a different feature, not a duplicate.
#[tauri::command]
pub fn nodes_duplicate(state: State<DbPool>, id: String) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::duplicate(c, &id))
}

/// The request configuration of one endpoint node.
#[tauri::command]
pub fn endpoint_detail_get(
    state: State<DbPool>,
    node_id: String,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_conn(&state, |c| crate::db::endpoint_details::get(c, &node_id))
}

/// Every endpoint's configuration in the workspace, as a flat list the
/// renderer keys by `nodeId`.
///
/// Separate from [`nodes_list`] because the tree never renders a method
/// or a url — it only needs name / type / order. Reading the detail table
/// for every node on boot would pull a column set most rows do not have,
/// and folding it into the tree read would be a join on every startup for
/// data the user may never open.
#[tauri::command]
pub fn endpoint_details_list(state: State<DbPool>) -> Result<Vec<EndpointDetail>, AppError> {
    // One query per project rather than a join: `endpoint_details` is
    // keyed by node id, so the only way to reach a project's endpoints is
    // through `nodes.root_id`. `list_by_root` already does that join, and
    // the project count is bounded by what the user has open.
    let roots = crate::db::with_conn(&state, crate::db::projects::list)?
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<String>>();
    let mut out = Vec::new();
    for root in roots {
        out.extend(crate::db::with_conn(&state, |c| {
            crate::db::endpoint_details::list_by_root(c, &root)
        })?);
    }
    Ok(out)
}

/// Patch an endpoint's request configuration. Separate from
/// [`nodes_update`] so renaming an endpoint does not rewrite the JSON
/// columns that did not change.
#[tauri::command]
pub fn endpoint_detail_update(
    state: State<DbPool>,
    node_id: String,
    patch: EndpointDetailPatch,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoint_details::update(c, &node_id, patch.clone())
    })
}
