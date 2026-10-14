use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;

use crate::db;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::dto::{BaseUrlDefCreateBody, BaseUrlDefSetDefaultBody};

// ----- baseUrlDefs --------------------------------------------------------
//
// A service belongs to a **project**, and the address says so: the
// create and set-default verbs hang off `/api/projects/:projectId/baseurls`,
// next to the list they extend. Only the id-scoped verbs stay under
// `/api/baseurl-defs/:defId`, because a rename or a delete names the
// service itself and needs no project context to find it.

/// Read every service of a project, flat.
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

/// Add a service to the project's list.
pub(crate) async fn base_url_defs_create(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefCreateBody>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let def = crate::domain::BaseUrlDefinition {
        // Same id policy as the Tauri command, one helper for both (see
        // `crate::dto::adopt_or_mint_id`): the renderer keys the project's
        // ★ and the per-(env × module) URL cells off this id before the
        // row exists.
        id: crate::dto::adopt_or_mint_id(body.id),
        name: body.name,
        description: body.description,
    };
    // The stored row comes back rather than the request's own copy: the
    // renderer swaps it into the project's list it already holds.
    let stored = db::with_tx(&s.pool, |c| {
        db::base_url_defs::create(c, &def, &project_id)?;
        db::base_url_defs::get(c, &def.id)
    })
    .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&stored).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn base_url_defs_update(
    State(s): State<Arc<WebState>>,
    Path(def_id): Path<String>,
    Json(patch): Json<crate::dto::BaseUrlDefPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `BaseUrlDefPatch` lives in `commands/dto.rs` (not `domain`) because
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

/// Point the project's ★ at one of its own services (`defId = null`
/// clears it) and echo the project back.
///
/// The ★ is `Project.defaultBaseUrlDefinitionId` — the pointer every
/// endpoint naming no service resolves against — so the caller is
/// handed the updated value instead of a `204` and a re-read.
pub(crate) async fn base_url_defs_set_default(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefSetDefaultBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let project = db::with_tx(&s.pool, |c| {
        db::base_url_defs::set_default(c, &project_id, body.def_id.as_deref())?;
        db::projects::get(c, &project_id)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&project).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "base_url_defs_test.rs"]
mod tests;
