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
// 服务属于**项目**，地址也这么说：create 与 set-default 挂在
// `/api/projects/:projectId/baseurls` 下，紧邻它们扩充的那个列表。只有按 id 的动词留在
// `/api/baseurl-defs/:defId`——改名或删除点名的是服务本身，找到它不需要项目上下文。

/// 读某项目的全部服务，打平。
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

/// 往项目的服务列表里加一个。
///
/// 编排住在 `crate::project_api`（server crate），Tauri 命令调的是同一个函数，故两条
/// transport 不会对「服务 id 归谁」各说各话。这一侧只负责线格式。
pub(crate) async fn base_url_defs_create(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefCreateBody>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    // 回存下的行而不是请求里那份副本：renderer 要拿它换掉自己手上已有的项目列表里的
    // 那一条。
    let stored = crate::project_api::create_base_url_def(
        &s.pool,
        &project_id,
        body.id,
        body.name,
        body.description,
    )
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
    // `BaseUrlDefPatch` 住在 `commands/dto.rs`（而非 `domain`），因为它没有其他调用方。
    // 在此复用它，好让线格式与 IPC 路径完全一致。
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

/// 把项目的 ★ 指向它自己的某个服务（`defId = null` 则清空），并回显项目。
///
/// ★ 就是 `Project.defaultBaseUrlDefinitionId`——任何没点名服务的 endpoint 都拿它
/// 解析——故直接回传更新后的值，而不是 `204` 加一次重读。
///
/// 编排住在 `crate::project_api`（server crate），Tauri 命令调的是同一个函数。
pub(crate) async fn base_url_defs_set_default(
    State(s): State<Arc<WebState>>,
    Path(project_id): Path<String>,
    Json(body): Json<BaseUrlDefSetDefaultBody>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let project =
        crate::project_api::set_default_base_url_def(&s.pool, &project_id, body.def_id.as_deref())
            .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&project).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "base_url_defs_test.rs"]
mod tests;
