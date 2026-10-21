//! collection 节点上的两段脚本：`GET` / `PATCH /api/nodes/:id/scripts`。
//!
//! 端点的脚本走 `/api/nodes/:id/endpoint-detail`（它们住在 `endpoint_details` 行里），
//! 这一对只管 collection——它住在 `node_scripts` 表。
//!
//! 没有配置脚本不是错误：`GET` 永远回一份全空的 `NodeScripts`，而不是 404。

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::Value;

use crate::db;
use crate::domain::NodeScriptsPatch;

use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `GET /api/nodes/:id/scripts`
pub(crate) async fn node_scripts_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let scripts =
        db::with_conn(&s.pool, |c| db::node_scripts::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&scripts).unwrap_or(Value::Null)))
}

/// `PATCH /api/nodes/:id/scripts`
pub(crate) async fn node_scripts_patch(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<NodeScriptsPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let scripts = db::with_tx(&s.pool, |c| db::node_scripts::set(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&scripts).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "node_scripts_test.rs"]
mod tests;
