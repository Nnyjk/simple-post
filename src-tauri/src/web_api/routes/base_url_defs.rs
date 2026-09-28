use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;

use crate::db;
use crate::error::AppError;

use super::super::{ensure_server_id, map_app_error, HttpErrorResponse, WebState};

use super::super::dto::BaseUrlDefSetDefaultBody;

// ----- baseUrlDefs --------------------------------------------------------
//
// `base_url_defs` live in their own table; we keep them reachable via
// two route families:
//   - `/api/projects/:projectId/baseurls[/default]` for project-scoped
//     creates and "set as project default" actions
//   - `/api/baseurl-defs/:defId` for id-scoped updates / deletes
//
// This mirrors the path the resource lives on in the renderer (a def
// "belongs to" a project) without forcing the TS caller to remember
// which table a delete hits.

/// Read the project's base-URL definitions.
///
/// The renderer does not normally call this: `Project` nests its
/// `baseUrlDefinitions`, so the store already has them after bootstrap.
/// It exists so a REST client that just created or patched a definition
/// can read the family back — previously the family offered POST / PATCH
/// / DELETE / set-default but no way to list.
pub(crate) async fn base_url_defs_list(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, |c| {
        db::base_url_defs::list_by_project(c, &project_id)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&rows).unwrap_or(Value::Null)))
}

pub(crate) async fn base_url_defs_create(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let def: crate::domain::BaseUrlDefinition = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::base_url_defs::create(c, &def, &project_id))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&def).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn base_url_defs_update(
    State(s): State<Arc<WebState>>,
    Path(def_id): Path<String>,
    Json(patch): Json<crate::commands::BaseUrlDefPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `BaseUrlDefPatch` lives in `commands.rs` (not `domain`) because
    // it has no other callers. Reusing it here keeps the wire shape
    // identical to the IPC path.
    let row = db::with_tx(&s.pool, |c| {
        db::base_url_defs::update(c, &def_id, patch.name.clone(), patch.description.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

pub(crate) async fn base_url_defs_delete(
    State(s): State<Arc<WebState>>,
    Path(def_id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::base_url_defs::delete(c, &def_id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn base_url_defs_set_default(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefSetDefaultBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Echo back the post-update project so the renderer can replace
    // its in-memory copy without an extra round-trip.
    let row = db::with_tx(&s.pool, |c| {
        db::base_url_defs::set_default(c, &project_id, body.def_id.as_deref())?;
        crate::db::projects::get(c, &project_id)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}
