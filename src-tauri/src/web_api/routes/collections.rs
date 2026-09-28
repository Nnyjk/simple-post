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

use super::super::dto::CollectionMoveBody;

// ----- collections --------------------------------------------------------

pub(crate) async fn collections_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("moduleId") {
        Some(module_id) => {
            db::with_conn(&s.pool, |c| db::collections::list_by_module(c, module_id))
                .map_err(map_app_error)?
        }
        None => db::with_conn(&s.pool, db::collections::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn collections_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let collection: crate::domain::Collection = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::collections::create(c, &collection)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&collection).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn collections_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::CollectionPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::collections::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn collections_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::collections::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn collections_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<CollectionMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::collections::move_many(
            c,
            &body.collection_ids,
            &body.parent_kind,
            &body.parent_id,
            body.index,
        )
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
