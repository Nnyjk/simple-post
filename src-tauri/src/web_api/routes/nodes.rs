//! REST handlers for the merged resource tree.
//!
//! Replaces the 19 paths `/api/modules/*`, `/api/collections/*` and
//! `/api/endpoints/*` carried between them. The collapse is the point:
//! one table, one set of handlers, and the branch that used to read
//! `"module"` or `"collection"` when computing a move's destination is
//! gone — `db::nodes::move_many` asks the parent's type instead.
//!
//! The two endpoint-only sub-resources (`/duplicate`, `/send`) stay, now
//! addressed under a node. They are actions on one request, not part of
//! the tree's structure.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput, HttpResult};

use super::super::{ensure_server_id, map_app_error, HttpErrorResponse, WebState};

use super::super::dto::{HttpSendBody, NodeCreateBody, NodeMoveBody};

/// `POST /api/nodes/:id/send` — endpoints only.
///
/// The browser names the endpoint, not a URL. This is the fix for the
/// previous `POST /api/http/send` shape, where any authenticated caller
/// could hand the server an arbitrary target — the cloud metadata
/// address `169.254.169.254` included, which made every `serve-web` /
/// Docker deployment an open proxy. `/api/http/send` still exists for
/// ad-hoc raw-URL pokes, but it is no longer what the UI uses.
pub(crate) async fn node_send(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<HttpResult>, HttpErrorResponse> {
    // The renderer's URL input only ever holds a path; the base URL is
    // composed from the endpoint + environment rows. An absolute URL in
    // the draft is therefore either a client bug or someone aiming the
    // server at a host of their choosing — which is precisely the SSRF
    // this route exists to remove. `join_base_url` would happily accept
    // it (a path carrying a scheme wins), so it is refused here, at the
    // shape boundary where the caller is an untrusted remote client.
    //
    // The Tauri command allows it: there the local user is the operator
    // and the absolute form is a legitimate thing to type. A caller who
    // genuinely wants an arbitrary target on this deployment can still
    // use `POST /api/http/send`, which is gated by the outbound policy.
    if let Some(u) = body.pointer("/overrides/url").and_then(|v| v.as_str()) {
        if reqwest::Url::parse(u).is_ok() {
            return Err(map_app_error(AppError::Validation(format!(
                "overrides.url must be a path relative to the endpoint's base URL, \
                 got an absolute URL `{u}`. Use POST /api/http/send for ad-hoc targets."
            ))));
        }
    }

    // `Json<Value>` accepts any JSON value, but `body["k"] = v` panics on a
    // non-object (serde_json's `index_or_insert` has `_ => panic!`). With
    // `panic = "abort"` in the release profile that turns a malformed
    // request body into a process abort for every other user, so the shape
    // is checked before the insert.
    let Value::Object(mut map) = body else {
        return Err(map_app_error(AppError::Validation(
            "request body must be a JSON object".into(),
        )));
    };
    map.insert("endpointId".to_string(), Value::String(id));
    let args: crate::commands::EndpointSendArgs = serde_json::from_value(Value::Object(map))
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    let result = crate::commands::send_endpoint_by_id(&s.pool, &args)
        .await
        .map_err(map_app_error)?;
    Ok(Json(result))
}

/// `POST /api/http/send` — the raw-URL escape hatch.
///
/// Not part of the tree; kept at its own path because it is not about a
/// stored node. Gated by the outbound policy, unlike `node_send` above.
pub(crate) async fn http_send(
    State(_s): State<Arc<WebState>>,
    Json(body): Json<HttpSendBody>,
) -> Result<Json<HttpResult>, HttpErrorResponse> {
    let input = HttpRequestInput {
        method: body.method,
        url: body.url,
        headers: body.headers,
        body: body.body,
        timeout_ms: body.timeout_ms,
        follow_redirects: body.follow_redirects,
        max_response_size_kb: body.max_response_size_kb,
    };
    // The function returns an `HttpResult` (success OR transport-level
    // failure wrapped in `ok=false`). Both serialise to JSON cleanly.
    Ok(Json(http_executor::execute(&input).await))
}

/// `GET /api/nodes`
///
/// `rootId` narrows to one project — the SPA always has an active one,
/// so this replaces three unfiltered list calls that used to pull the
/// whole database regardless. `type` and `q` are optional filters; `q`
/// searches endpoint name, url and description, which is why it reaches
/// into `endpoint_details` rather than matching on the node alone.
pub(crate) async fn nodes_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("q") {
        Some(q) => db::with_conn(&s.pool, |c| db::nodes::search(c, q)).map_err(map_app_error)?,
        None => match params.get("type") {
            Some(t) => {
                let kind = ResourceNodeType::parse(t).ok_or_else(|| {
                    map_app_error(AppError::Validation(format!("unknown node type {t:?}")))
                })?;
                let root_id = params.get("rootId").map(String::as_str).ok_or_else(|| {
                    map_app_error(AppError::Validation("type filter requires rootId".into()))
                })?;
                db::with_conn(&s.pool, |c| {
                    db::nodes::list_by_root_and_type(c, root_id, kind)
                })
                .map_err(map_app_error)?
            }
            None => match params.get("rootId") {
                Some(root_id) => db::with_conn(&s.pool, |c| db::nodes::list_by_root(c, root_id))
                    .map_err(map_app_error)?,
                // No active project yet during first paint. Returning
                // the whole tree mirrors what the old three list calls
                // did, so the SPA can still render something.
                None => db::with_conn(&s.pool, db::nodes::list_all).map_err(map_app_error)?,
            },
        },
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

/// `GET /api/nodes/:id`
pub(crate) async fn nodes_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::nodes::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

/// `POST /api/nodes`
///
/// An endpoint arrives as one body carrying both halves: the structural
/// node and its `detail`. They are written in a single transaction, and
/// `db::nodes::create` refuses a mismatched pair — an endpoint with no
/// payload, or a collection carrying one — rather than leaving a tree
/// the reader cannot open.
pub(crate) async fn nodes_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let parsed: NodeCreateBody = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;

    let node = parsed.node;
    // A child node's owner is implied by its parent. Deriving it here
    // means a caller only has to name the parent, and the two can never
    // disagree — the same rule and the same reason as the Tauri
    // command's empty-`root_id` branch.
    let mut root_id = node.root_id.clone().unwrap_or_default();
    if root_id.is_empty() {
        if let Some(parent) = &node.parent_id {
            root_id = db::with_conn(&s.pool, |c| db::nodes::root_id_of(c, parent))
                .map_err(map_app_error)?;
        }
    }
    let node = node.into_node(&root_id);

    // Expand the optional detail block here rather than at the DTO, so a
    // half-filled endpoint ("new request" sends just a url) defaults the
    // same way the Tauri command defaults it.
    let detail = parsed.detail.map(|d| d.into_detail(&node.id));

    let created = db::with_tx(&s.pool, |c| db::nodes::create(c, &node, detail.as_ref()))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}

/// `PATCH /api/nodes/:id` — structural columns only.
///
/// Endpoint configuration has its own route. Mixing them would make a
/// name rename rewrite JSON columns that did not change.
pub(crate) async fn nodes_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ResourceNodePatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `with_tx` wants `Fn`, so the patch is cloned rather than moved.
    let row = db::with_tx(&s.pool, |c| db::nodes::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

/// `DELETE /api/nodes/:id`
///
/// Takes the whole subtree: `nodes.parent_id` self-references with
/// `ON DELETE CASCADE`, which is the same one-statement teardown the
/// old three-level foreign key chain gave.
pub(crate) async fn nodes_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::nodes::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/nodes/move`
///
/// `parentId` is null for "top of the project", which is legal only for
/// a module. The old body carried a separate `parentKind` because the
/// destination was chosen from two different tables; the parent's type
/// answers that now.
pub(crate) async fn nodes_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<NodeMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::nodes::move_many(c, &body.node_ids, body.parent_id.as_deref(), body.index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- endpoint-only sub-resources ---------------------------------------

/// `GET /api/endpoint-details` — every endpoint's configuration, flat.
///
/// The renderer's counterpart to `nodes_list`: the tree reads structure
/// and the HTTP tester reads configuration, and the two are different
/// enough that joining them on every boot would be a cost paid for data
/// the tree never draws. Keyed by `nodeId` on the client.
pub(crate) async fn endpoint_details_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let projects = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    let mut out = Vec::new();
    for p in projects {
        out.extend(
            db::with_conn(&s.pool, |c| db::endpoint_details::list_by_root(c, &p.id))
                .map_err(map_app_error)?,
        );
    }
    Ok(Json(
        serde_json::to_value(&out).unwrap_or_else(|_| json!([])),
    ))
}

/// `GET /api/nodes/:id/endpoint-detail`
pub(crate) async fn node_endpoint_detail_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let d = db::with_conn(&s.pool, |c| db::endpoint_details::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&d).unwrap_or(Value::Null)))
}

/// `PATCH /api/nodes/:id/endpoint-detail`
pub(crate) async fn node_endpoint_detail_patch(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::EndpointDetailPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let d = db::with_tx(&s.pool, |c| {
        db::endpoint_details::update(c, &id, patch.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&d).unwrap_or(Value::Null)))
}

/// Body for `POST /api/nodes/:id/duplicate`.
///
/// Optional in its entirety — the whole point of the body is the id, and
/// a caller that has no local copy to stay consistent with (a script,
/// curl) can omit it and let the server mint one. Defined here rather
/// than in `dto.rs` so the route's whole contract sits next to the
/// handler that enforces it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeDuplicateBody {
    /// The id to write the copy under, honoured verbatim.
    ///
    /// `#[serde(default)]` rather than a bare `Option<String>` because
    /// the browser sends `null` when it has no id to offer: "absent" and
    /// "explicitly null" both mean *mint one here*, and the SPA sending
    /// null must not turn into a 400.
    #[serde(default)]
    pub new_id: Option<String>,
}

/// `POST /api/nodes/:id/duplicate` — endpoints only.
///
/// Delegates to `db::nodes::duplicate`, which copies the node and its
/// detail in one transaction and refuses a non-endpoint up front rather
/// than producing an empty collection that looks like a half-finished
/// copy.
pub(crate) async fn node_duplicate(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    // `Option<Json<_>>`, not `Json<_>`: the body is genuinely optional,
    // and a required extractor would 400 every caller written before the
    // SPA started sending an id.
    body: Option<Json<NodeDuplicateBody>>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let new_id = body.and_then(|Json(b)| b.new_id);
    let created = db::with_tx(&s.pool, |c| db::nodes::duplicate(c, &id, new_id.as_deref()))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}
