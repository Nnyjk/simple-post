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
    // 镜像 Tauri 命令：回裸的 `Permission`（经
    // `#[serde(rename_all = "lowercase")]` 序列化为小写字符串）。TS 侧在 IPC 那边已经
    // 持有这份契约。
    let perm =
        db::with_conn(&s.pool, |c| db::permissions::get(c, &project_id)).map_err(map_app_error)?;
    Ok(Json(perm.as_str().to_string()))
}

pub(crate) async fn permissions_set(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<PermissionPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // `Permission::parse` 是可接受取值的唯一事实来源——调用点拼错会响亮地 400，而不是
    // 写进一行垃圾数据。
    let perm = crate::domain::Permission::parse(&body.permission).ok_or_else(|| {
        map_app_error(AppError::Validation(format!(
            "unknown permission: {}",
            body.permission
        )))
    })?;
    db::with_tx(&s.pool, |c| db::permissions::set(c, &project_id, perm)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
