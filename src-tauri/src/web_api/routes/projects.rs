use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::db;
use crate::error::AppError;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::ensure_server_id;

pub(crate) async fn projects_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn projects_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::projects::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

pub(crate) async fn projects_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let project: crate::domain::Project = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::projects::create(c, &project)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&project).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn projects_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ProjectPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` takes an `Fn` closure (not `FnOnce`), so we can't
    // move `patch` in — clone it. The struct is a small flat object.
    let row = db::with_tx(&s.pool, |c| db::projects::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

pub(crate) async fn projects_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::projects::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
