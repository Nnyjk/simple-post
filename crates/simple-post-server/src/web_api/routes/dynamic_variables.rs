//! 动态变量的 REST handler。
//!
//! 七条路由：列表、新建、改、删、解析、写覆盖值、强制重取。逻辑全部在
//! `crate::dynamic_api`，故 Tauri 命令与这些路由走同一套实现。
//!
//! 定义挂在**项目**里：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。
//! 列表与解析带一个可选的项目参数：不带就是跨项目那一路。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;

use crate::domain::DynamicVariablePatch;

use super::super::dto::{
    DynamicVariableCreateBody, DynamicVariableOverrideBody, DynamicVariablePreviewBody,
    DynamicVariableRefreshBody, DynamicVariableResolveBody,
};
use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `GET /api/dynamic-variables` —— 列出定义，带当前值与缓存状态。
///
/// **不取数**：读的是缓存行。取数会发出站请求，而这是一次纯读。
///
/// `projectId` 缺席时列出全部项目；给了就只列那个项目里的。
///
/// `environmentId` 是调用方此刻选中的环境：只对继承取数环境的定义起作用，缓存行按实际
/// 生效的环境取。缺席 = 调用方没有环境。
pub(crate) async fn dynamic_variables_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let project_id = params.get("projectId").filter(|id| !id.is_empty());
    let environment_id = params.get("environmentId").filter(|id| !id.is_empty());
    let rows = crate::dynamic_api::list(
        &s.pool,
        project_id.map(String::as_str),
        environment_id.map(String::as_str),
    )
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
        project_id: Some(body.project_id),
        name: body.name,
        description: body.description,
        source_endpoint_id: body.source_endpoint_id,
        source_inline_json: body.source_inline_json,
        source_environment_id: body.source_environment_id,
        extract_path: body.extract_path,
        transform_script: body.transform_script,
        value_mode: body.value_mode,
        ttl_seconds: body.ttl_seconds,
        stale_fallback: body.stale_fallback,
        enabled: body.enabled,
        sort_order: body.sort_order,
    };
    let stored = crate::dynamic_api::create(&s.pool, opts).map_err(map_app_error)?;
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
///
/// `projectId` 缺席时跨项目找：同名定义出现在多个项目里时，按歧义处理，不猜。
///
/// `environmentId` 是调用方此刻选中的环境，只对继承取数环境的定义起作用。
pub(crate) async fn dynamic_variables_resolve(
    State(s): State<Arc<WebState>>,
    Json(body): Json<DynamicVariableResolveBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let resolution = crate::dynamic_api::resolve(
        &s.pool,
        body.project_id.as_deref(),
        &body.names,
        body.force.unwrap_or(false),
        body.environment_id.as_deref(),
    )
    .await
    .map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(&resolution).unwrap_or(Value::Null),
    ))
}

/// `POST /api/dynamic-variables/{id}/override` —— 写覆盖值。`value = null` 清除它。
///
/// `environmentId` 是调用方此刻选中的环境：覆盖值落在（定义 × 实际生效的环境）那一格。
pub(crate) async fn dynamic_variables_set_override(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(body): Json<DynamicVariableOverrideBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    crate::dynamic_api::set_override(
        &s.pool,
        &id,
        body.value,
        body.expires_at,
        body.forever.unwrap_or(false),
        body.environment_id.as_deref(),
    )
    .map_err(map_app_error)?;
    let view = crate::dynamic_api::get(&s.pool, &id, body.environment_id.as_deref())
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&view).unwrap_or(Value::Null)))
}

/// `POST /api/dynamic-variables/{id}/refresh` —— 强制重新取一次数。
///
/// body 可选：继承取数环境的定义要拿调用方此刻选中的环境，故它带一个可选的
/// `environmentId`；不提环境的老调用方照旧能打这个路由。
///
/// 空 body 一律当「没有 body」，无论带不带 `Content-Type: application/json`：`Bytes` 不会
/// 在校验阶段把空内容拒掉，判定权留在处理函数里。
pub(crate) async fn dynamic_variables_refresh(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, HttpErrorResponse> {
    let environment_id = if body.is_empty() {
        None
    } else {
        let body: DynamicVariableRefreshBody = serde_json::from_slice(&body).map_err(|e| {
            map_app_error(crate::error::AppError::Validation(format!(
                "请求体不是合法的 JSON：{e}"
            )))
        })?;
        body.environment_id
    };
    let status = crate::dynamic_api::refresh(&s.pool, &id, environment_id.as_deref())
        .await
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&status).unwrap_or(Value::Null)))
}

/// `POST /api/dynamic-variables/preview` —— 用表单里当前这份草稿真取一次数。
///
/// 它会真的发出一次请求：不修改本地持久状态（缓存 / 覆盖值 / 环境变量 / 响应历史），
/// 但对端看得见这次调用——预览一个非幂等的 POST 两次，对端就执行两次。别把它说成只读或幂等。
/// 唯一的写入例外是 OAuth2 令牌：数据源配了 OAuth2 且缓存里没有可用令牌时，
/// `oauth2::ensure_token` 会写一次 token 缓存，那是发送凭据而不是变量的值。
///
/// 取数失败是唯一会让整条请求失败的路径（没响应就没有正文可看）；抽取与处理脚本的失败、
/// 非 2xx 的响应、序列化后超过上限的值，都落在结果的对应字段里，正文照给。
///
/// `environmentId` 是调用方此刻选中的环境，只对继承取数环境的草稿起作用；它与
/// `sourceEnvironmentId`（草稿点名的那一个环境）是两件事。
pub(crate) async fn dynamic_variables_preview(
    State(s): State<Arc<WebState>>,
    Json(body): Json<DynamicVariablePreviewBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let draft = crate::dynamic_api::PreviewDraft {
        name: body.name,
        source_endpoint_id: body.source_endpoint_id,
        source_inline_json: body.source_inline_json,
        source_environment_id: body.source_environment_id,
        extract_path: body.extract_path,
        transform_script: body.transform_script,
    };
    let result = crate::dynamic_api::preview(&s.pool, draft, body.environment_id.as_deref())
        .await
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&result).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "dynamic_variables_test.rs"]
mod tests;
