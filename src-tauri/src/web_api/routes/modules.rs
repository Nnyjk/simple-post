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

use super::super::dto::ModuleMoveBody;

// ----- modules ------------------------------------------------------------

pub(crate) async fn modules_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Filter by project when given; otherwise fall back to `list_all`
    // so the SPA bootstrap path (no active project yet) still gets a
    // useful dataset to filter client-side.
    let rows = match params.get("projectId") {
        Some(project_id) => db::with_conn(&s.pool, |c| db::modules::list_by_project(c, project_id))
            .map_err(map_app_error)?,
        None => db::with_conn(&s.pool, db::modules::list_all).map_err(map_app_error)?,
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn modules_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    // Accept the full `Module` struct. `Module` has no server-side
    // timestamp fields so `#[serde(default)]` on those isn't needed —
    // but `id` is server-stamped via `ensure_server_id` so the SPA can
    // ship name/description/sortOrder/etc. without a hand-rolled id.
    let module: crate::domain::Module = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::modules::create(c, &module)).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&module).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn modules_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ModulePatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` takes an `Fn` closure (not `FnOnce`), so we can't
    // move `patch` in — clone it. Same idiom as `projects_update`.
    let row = db::with_tx(&s.pool, |c| db::modules::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn modules_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::modules::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn modules_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<ModuleMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        // `r#move` is the raw identifier (`move` is a Rust keyword).
        db::modules::r#move(c, &body.module_id, body.target_index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
