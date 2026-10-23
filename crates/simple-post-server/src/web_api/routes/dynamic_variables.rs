//! 动态变量的 REST handler。
//!
//! 七条路由：列表、新建、改、删、解析、写覆盖值、强制重取。逻辑全部在
//! `crate::dynamic_api`，故 Tauri 命令与这些路由走同一套实现。

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;

use crate::domain::DynamicVariablePatch;

use super::super::dto::{
    DynamicVariableCreateBody, DynamicVariableListQuery, DynamicVariableOverrideBody,
    DynamicVariableRefreshBody, DynamicVariableResolveBody,
};
use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `GET /api/dynamic-variables?projectId=` —— 列出定义，带当前值与缓存状态。
///
/// **不取数**：读的是缓存行。取数会发出站请求，而这是一次纯读。
pub(crate) async fn dynamic_variables_list(
    State(s): State<Arc<WebState>>,
    Query(query): Query<DynamicVariableListQuery>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows =
        crate::dynamic_api::list(&s.pool, &query.project_id, query.environment_id.as_deref())
            .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&rows).unwrap_or(Value::Null)))
}

/// `POST /api/dynamic-variables` —— 新建一条定义。
pub(crate) async fn dynamic_variables_create(
    State(s): State<Arc<WebState>>,
    Json(body): Json<DynamicVariableCreateBody>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let opts = crate::dynamic_api::CreateOpts {
        id: body.id,
        name: body.name,
        description: body.description,
        source_endpoint_id: body.source_endpoint_id,
        source_inline_json: body.source_inline_json,
        extract_path: body.extract_path,
        value_mode: body.value_mode,
        ttl_seconds: body.ttl_seconds,
        stale_fallback: body.stale_fallback,
        enabled: body.enabled,
        sort_order: body.sort_order,
    };
    let stored =
        crate::dynamic_api::create(&s.pool, &body.project_id, opts).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&stored).unwrap_or(Value::Null)),
    ))
}

/// `PATCH /api/dynamic-variables/{id}` —— 改一条定义。
pub(crate) async fn dynamic_variables_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<DynamicVariablePatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = crate::dynamic_api::update(&s.pool, &id, patch).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

/// `DELETE /api/dynamic-variables/{id}` —— 删一条定义。缓存行由外键带走。
pub(crate) async fn dynamic_variables_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    crate::dynamic_api::delete(&s.pool, &id).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/dynamic-variables/resolve` —— 解析一批名字。
///
/// 这一步会真的发出站请求（去数据源取数），故它与「读」是两种不同的代价。
pub(crate) async fn dynamic_variables_resolve(
    State(s): State<Arc<WebState>>,
    Json(body): Json<DynamicVariableResolveBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let resolution = crate::dynamic_api::resolve(
        &s.pool,
        &body.project_id,
        body.environment_id.as_deref(),
        &body.names,
        body.force.unwrap_or(false),
    )
    .await
    .map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(&resolution).unwrap_or(Value::Null),
    ))
}

/// `POST /api/dynamic-variables/{id}/override` —— 写覆盖值。`value = null` 清除它。
pub(crate) async fn dynamic_variables_set_override(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(body): Json<DynamicVariableOverrideBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    crate::dynamic_api::set_override(
        &s.pool,
        &id,
        body.environment_id.as_deref(),
        body.value,
        body.expires_at,
        body.forever.unwrap_or(false),
    )
    .map_err(map_app_error)?;
    let view = crate::dynamic_api::get(&s.pool, &id, body.environment_id.as_deref())
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&view).unwrap_or(Value::Null)))
}

/// `POST /api/dynamic-variables/{id}/refresh` —— 强制重新取一次数。
pub(crate) async fn dynamic_variables_refresh(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(body): Json<DynamicVariableRefreshBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let status = crate::dynamic_api::refresh(
        &s.pool,
        &body.project_id,
        &id,
        body.environment_id.as_deref(),
    )
    .await
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&status).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "dynamic_variables_test.rs"]
mod tests;
