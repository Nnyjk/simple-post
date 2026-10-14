use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::db;
use crate::error::AppError;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::dto::PermissionPutBody;

// ----- permissions --------------------------------------------------------

pub(crate) async fn permissions_get(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
) -> Result<Json<String>, HttpErrorResponse> {
    // Mirror the Tauri command: return the bare `Permission` (which
    // serializes to a lowercase string via
    // `#[serde(rename_all = "lowercase")]`). The TS caller already
    // has this contract on the IPC side.
    let perm =
        db::with_conn(&s.pool, |c| db::permissions::get(c, &project_id)).map_err(map_app_error)?;
    Ok(Json(perm.as_str().to_string()))
}

pub(crate) async fn permissions_set(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<PermissionPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // `Permission::parse` is the single source of truth for the
    // accepted values — a typo at the call site fails loudly with
    // 400 instead of writing a bogus row.
    let perm = crate::domain::Permission::parse(&body.permission).ok_or_else(|| {
        map_app_error(AppError::Validation(format!(
            "unknown permission: {}",
            body.permission
        )))
    })?;
    db::with_tx(&s.pool, |c| db::permissions::set(c, &project_id, perm)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
