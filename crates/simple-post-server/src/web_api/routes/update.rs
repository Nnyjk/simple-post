//! 更新检查 / 更新执行的三条路由。
//!
//! 薄壳：解析 query、把 `WebState.password` 当作「是不是口令模式」交给
//! [`crate::update_api`]，判定的规则全在那边。三条路都**不在** `auth::is_public_path`
//! 名单里，故口令模式下一律要 bearer。
//!
//! `check` 无论成功还是失败都回 200（失败信息在响应体的 `error` 字段里）：渲染层的
//! `invokeCmd` 把非 2xx 变成异常，异常里只剩一句 HTTP 状态，读不出「上游限流」与
//! 「这台机器断网」的区别。`apply` 是例外——它的 403 / 409 就是「没执行」本身。

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::super::WebState;
use crate::update_api::{self, ApplyError, UpdateApplyResult, UpdateDeployment};
use simple_post_core::update::UpdateCheck;

/// `GET /api/update/check` 的查询串。只认 `force`，取 `1` 或 `true`（其余一律当没有）。
#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCheckQuery {
    force: Option<String>,
}

impl UpdateCheckQuery {
    fn force(&self) -> bool {
        matches!(self.force.as_deref(), Some("1") | Some("true"))
    }
}

pub(crate) async fn update_check(Query(query): Query<UpdateCheckQuery>) -> Json<UpdateCheck> {
    Json(update_api::check(query.force()).await)
}

pub(crate) async fn update_deployment(
    State(state): State<Arc<WebState>>,
) -> Json<UpdateDeployment> {
    Json(update_api::deployment(state.password.is_some()))
}

pub(crate) async fn update_apply(
    State(state): State<Arc<WebState>>,
) -> Result<Json<UpdateApplyResult>, (StatusCode, Json<Value>)> {
    update_api::apply(state.password.is_some())
        .await
        .map(Json)
        .map_err(apply_error)
}

/// 两道门的拒绝与「已经有一次在跑」共用 `{ code, message }` 形状，与其余 `/api/*`
/// 的错误体一致。
fn apply_error(error: ApplyError) -> (StatusCode, Json<Value>) {
    let status = match error {
        ApplyError::Disabled => StatusCode::FORBIDDEN,
        ApplyError::Busy => StatusCode::CONFLICT,
    };
    (
        status,
        Json(json!({ "code": error.code(), "message": error.message() })),
    )
}
