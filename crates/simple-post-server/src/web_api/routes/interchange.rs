//! 导入 / 导出的 REST 端点。全部是薄壳：参数反序列化之后调
//! [`crate::interchange_api`]，那里才是逻辑。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::dto::{
    ExportDocumentDto, ExportDocumentOpts, ExportPreviewDto, ExportPreviewOpts, ImportCommitDto,
    ImportCommitOpts, ImportPreviewDto, ImportPreviewOpts,
};
use crate::interchange_api;
use crate::web_api::{map_app_error, HttpErrorResponse, WebState};

/// 解析一份外来文档并给出可勾选的中间态。**不写库。**
pub async fn interchange_import_preview(
    State(state): State<Arc<WebState>>,
    Json(opts): Json<ImportPreviewOpts>,
) -> Result<Json<ImportPreviewDto>, HttpErrorResponse> {
    let out = interchange_api::import_preview(&state.pool, &opts)
        .await
        .map_err(map_app_error)?;
    Ok(Json(out))
}

/// 把勾中的条目落进目标模块。树与 Postman 环境在**同一个事务**里。
pub async fn interchange_import_commit(
    State(state): State<Arc<WebState>>,
    Json(opts): Json<ImportCommitOpts>,
) -> Result<Json<ImportCommitDto>, HttpErrorResponse> {
    let out = interchange_api::import_commit(&state.pool, &opts).map_err(map_app_error)?;
    Ok(Json(out))
}

/// 导出候选清单。**不写库。**
pub async fn interchange_export_preview(
    State(state): State<Arc<WebState>>,
    Json(opts): Json<ExportPreviewOpts>,
) -> Result<Json<ExportPreviewDto>, HttpErrorResponse> {
    let out = interchange_api::export_preview(&state.pool, &opts).map_err(map_app_error)?;
    Ok(Json(out))
}

/// 按选定的环境与勾选的条目产出 OpenAPI 文档。**不写库。**
pub async fn interchange_export_document(
    State(state): State<Arc<WebState>>,
    Json(opts): Json<ExportDocumentOpts>,
) -> Result<Json<ExportDocumentDto>, HttpErrorResponse> {
    let out = interchange_api::export_document(&state.pool, &opts).map_err(map_app_error)?;
    Ok(Json(out))
}
