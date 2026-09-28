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

use super::super::dto::{EnvRenameVarBody, EnvSetBaseUrlBody, EnvSetVarBody, SetActiveEnvBody};

// ----- environments -------------------------------------------------------

pub(crate) async fn environments_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("projectId") {
        Some(project_id) => db::with_conn(&s.pool, |c| {
            db::environments::list_by_project(c, project_id)
        })
        .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::environments::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn environments_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let env: crate::domain::Environment = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::environments::create(c, &env)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&env).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn environments_update(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(patch): Json<crate::domain::EnvironmentPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::update(c, &env_id, patch.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn environments_delete(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::environments::delete(c, &env_id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn environments_set_var(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(body): Json<EnvSetVarBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::set_var(c, &env_id, &body.key, body.value.as_deref())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn environments_rename_var(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(body): Json<EnvRenameVarBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::rename_var(c, &env_id, &body.from_key, &body.to_key)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn environments_set_baseurl(
    State(s): State<Arc<WebState>>,
    Path(env_id): Path<String>,
    Json(body): Json<EnvSetBaseUrlBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| {
        db::environments::set_base_url(c, &env_id, &body.def_id, body.url.as_deref())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn environments_set_active(
    State(s): State<Arc<WebState>>,
    Json(body): Json<SetActiveEnvBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_tx(&s.pool, |c| db::environments::set_active(c, &body.env_id))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}
