use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::db;
use crate::error::AppError;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::ensure_server_id;

pub(crate) async fn projects_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    Ok(Json(
        serde_json::to_value(rows).unwrap_or_else(|_| json!([])),
    ))
}

pub(crate) async fn projects_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::projects::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

/// 创建项目，并种下它的默认环境。
///
/// 有两种 body 形状，都仍在使用：
///
/// - `{ "name": …, "opts": { description, color } }` —— `invokeCmd(COMMANDS.projectsCreate, …)`
///   发的就是这个，因为 `WEB_ROUTES.projectsCreate` 没有 `body:` mapper，参数对象*就是*
///   payload。这是 SPA 自己的契约。
/// - 打平的 `Project` —— 文档化的公共 v1 形状，外部 REST 客户端使用。
///
/// 两者都解析：`Project` 没有 `opts` 字段，而 serde 会静默丢弃未知键，于是按 `Project`
/// 解析会让 create 表单的 description 与 colour 全被丢掉，只剩 name 返回。
///
/// `opts` 在场时优先，否则从打平的字段里读。base URL 不在其中：它不是项目属性，
/// `Project.base_url` 是遗留字段且从不持久化。
///
/// 编排住在 `crate::project_api`（server crate），Tauri 命令调的是同一个函数，故两条
/// transport 不会对「建项目种了什么」各说各话。这一侧只负责线格式：两种 body 形状的
/// 挑选、`name` 缺席时的报错，以及 `ensure_server_id` 补 id。
pub(crate) async fn projects_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let opts: crate::dto::CreateProjectOpts = match body.get("opts") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?,
        None => crate::dto::CreateProjectOpts {
            description: body
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            color: body
                .get("color")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| map_app_error(AppError::Validation("missing field 'name'".to_string())))?;

    ensure_server_id(&mut body);
    let id = body.get("id").and_then(Value::as_str).map(str::to_string);

    let created =
        crate::project_api::create_project(&s.pool, id, name, opts).map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}

pub(crate) async fn projects_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ProjectPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `db::with_tx` 收 `Fn` 闭包（不是 `FnOnce`），故不能把 `patch` 移进去——克隆一下。
    // 这个结构体是个小的扁平对象。
    let row = db::with_tx(&s.pool, |c| db::projects::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(Value::Null)))
}

pub(crate) async fn projects_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::projects::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "projects_test.rs"]
mod tests;
