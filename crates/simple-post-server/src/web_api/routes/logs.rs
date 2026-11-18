//! 日志导出与前端上报的两条路由。
//!
//! 薄壳：判定与文本都在 [`crate::log_api`]。两条都**不在** `auth::is_public_path`
//! 名单里：日志可能含凭据与内部地址，口令模式下必须带 bearer。

use axum::{extract::Query, http::StatusCode, Json};
use serde::Deserialize;

use super::super::{map_app_error, HttpErrorResponse};
use crate::log_api::{self, LogExport};

/// `GET /api/logs/export` 的查询串。缺省按 `all`。
#[derive(Debug, Default, Deserialize)]
pub(crate) struct LogExportQuery {
    scope: Option<String>,
}

pub(crate) async fn logs_export(
    Query(query): Query<LogExportQuery>,
) -> Result<Json<LogExport>, HttpErrorResponse> {
    log_api::export(query.scope.as_deref().unwrap_or("all"))
        .map(Json)
        .map_err(map_app_error)
}

/// `POST /api/logs/client` 的请求体。字段名与 Tauri 命令参数逐字一致，故渲染层两条传输
/// 发的是同一个对象。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogClientEventBody {
    level: String,
    source: String,
    message: String,
    detail: Option<String>,
}

pub(crate) async fn logs_client(
    Json(body): Json<LogClientEventBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    log_api::append_client_event(
        &body.level,
        &body.source,
        &body.message,
        body.detail.as_deref(),
    )
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "logs_test.rs"]
mod tests;
