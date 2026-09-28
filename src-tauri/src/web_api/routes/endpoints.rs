use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::db;
use crate::error::AppError;

use super::super::{ensure_server_id, map_app_error, HttpErrorResponse, WebState};

use crate::http_executor::{self, HttpRequestInput, HttpResult};

use super::super::dto::{EndpointMoveBody, EndpointWrite, EndpointsSearchBody, HttpSendBody};

/// Send a stored endpoint by id.
///
/// The browser names the endpoint, not a URL. This is the fix for the
/// previous `POST /api/http/send` shape, where any authenticated caller
/// could hand the server an arbitrary target — the cloud metadata address
/// `169.254.169.254` included, which made every `serve-web` / Docker
/// deployment an open proxy. `/api/http/send` still exists for ad-hoc
/// raw-URL pokes, but it is no longer what the UI uses.
pub(crate) async fn endpoint_send(
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

// ----- endpoints ----------------------------------------------------------

pub(crate) async fn endpoints_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Filter by collection when given; otherwise `list_all`. We don't
    // honour `projectId`/`moduleId` here — those would need additional
    // db overloads; callers that want project- or module-scoped walks
    // can filter the unfiltered list client-side until a richer db
    // function lands.
    let rows = match params.get("collectionId") {
        Some(collection_id) => db::with_conn(&s.pool, |c| {
            db::endpoints::list_by_collection(c, collection_id)
        })
        .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::endpoints::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn endpoints_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::endpoints::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn endpoints_search(
    State(s): State<Arc<WebState>>,
    Json(body): Json<EndpointsSearchBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `tags.as_deref()` lets the DB layer accept `&[String]` without
    // moving the `Vec` out of the body.
    let tags_slice = body.tags.as_deref();
    let rows = db::with_conn(&s.pool, |c| {
        db::endpoints::search(c, body.query.as_deref(), body.method.as_deref(), tags_slice)
    })
    .map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn endpoints_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    // `EndpointWrite` covers the caller-side fields; the server fills
    // `id` (when omitted), `created_at`, and `updated_at`. We deserialize
    // as `EndpointWrite` for typed safety of nested structs (params,
    // headers, body, auth) and then promote to the full `Endpoint` to
    // hand off to the db layer.
    ensure_server_id(&mut body);
    let write: EndpointWrite = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    let now = crate::domain::now_ms();
    let endpoint = crate::domain::Endpoint {
        id: write.id,
        collection_id: write.collection_id,
        name: write.name,
        description: write.description,
        method: write.method,
        url: write.url,
        environment_id: write.environment_id,
        base_url_definition_id: write.base_url_definition_id,
        base_url_override: write.base_url_override,
        params: write.params,
        headers: write.headers,
        body: write.body,
        auth: write.auth,
        docs: write.docs,
        notes: write.notes,
        tags: write.tags,
        sort_order: write.sort_order,
        created_at: now,
        updated_at: now,
    };
    db::with_tx(&s.pool, |c| db::endpoints::create(c, &endpoint)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&endpoint).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn endpoints_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::EndpointPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::endpoints::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn endpoints_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::endpoints::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn endpoints_duplicate(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<String>, HttpErrorResponse> {
    let new_id =
        db::with_tx(&s.pool, |c| db::endpoints::duplicate(c, &id)).map_err(map_app_error)?;
    Ok(Json(new_id))
}

pub(crate) async fn endpoints_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<EndpointMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::endpoints::move_many(c, &body.endpoint_ids, &body.collection_id, body.index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
