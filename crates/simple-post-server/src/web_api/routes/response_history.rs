//! 持久化响应历史条的 REST handler。
//!
//! 三个 `response_history_*` Tauri 命令的浏览器 transport。这里刻意很薄：上限
//! （每 endpoint 10 行）、256 KiB body 截断与行映射全都住在
//! `db::response_history`，故 IPC 路径与本路径不会在这两条易被 subtly 弄错的规则
//! 上漂移。
//!
//! 值得说一次的边界划分：**每个**动词都指向单个 endpoint——列表是因为它画的是某个
//! endpoint 的条，两个写是因为它们作用的就是那些行。此处没有任何 project 维度的
//! 东西：加一段 `:projectId` 只会引入一个服务端随后必须信任的 scope，而
//! `endpoint_id` 已随 `nodes` 级联删除，写入路径检测不出项目不匹配。

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db;
use crate::domain::StoredResponse;

use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `GET /api/response-history/:endpointId` —— 某个 endpoint 的行，最新在前。
///
/// 与下面的 `DELETE` 共用路径，故这两个作用于单个 endpoint 的动词在 URL 里读起来
/// 一样。
///
/// 什么也没存的 id 回 `200 []` 而非 `404`：这条路由最常见的请求就是「用户还没发过请求
/// 的 endpoint」的首次查询，而那是空历史而非缺失资源——回 404 会让「全新 endpoint」在
/// 两条 transport 上都长得像失败。
pub(crate) async fn response_history_list(
    State(s): State<Arc<WebState>>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, |c| {
        db::response_history::list_by_endpoint(c, &endpoint_id)
    })
    .map_err(map_app_error)?;
    // 序列化失败时给 `[]` 而非 `null`：这份行集是 renderer 会往里追加的列表，那里的
    // `null` 会被读成「服务端根本没有历史表」而不是「这次读失败了」。
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

/// `POST /api/response-history` 的 body。
///
/// 定义在此而非 `dto.rs`，理由同 `NodeDuplicateBody`：一个双字段路由的完整契约属于
/// 强制它的 handler，且没有第二个消费方值得为此扩大 `dto.rs`。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseHistoryAppendBody {
    pub endpoint_id: String,
    /// renderer 经 adapter 之后的 `ApiResponse` 形状。直接反序列化成 domain 类型，
    /// 故只有一份线格式契约，而不是一个要逐字段翻译的 REST DTO。
    pub response: StoredResponse,
}

/// `POST /api/response-history` —— 存一条响应。
///
/// 回 `201` 而非 `200`：这次调用建出了一行带服务端铸造 id 与服务端盖的 `createdAt`
/// 的记录，两者都在返回的 entry 里。`200` 会说「什么都没建」却递上一行确实建出来
/// 的记录。
pub(crate) async fn response_history_append(
    State(s): State<Arc<WebState>>,
    Json(body): Json<ResponseHistoryAppendBody>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let entry = db::with_tx(&s.pool, |c| {
        db::response_history::append(c, &body.endpoint_id, &body.response)
    })
    .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&entry).unwrap_or(Value::Null)),
    ))
}

/// `DELETE /api/response-history/:endpointId`
///
/// 回 `{ "removed": n }` 而非邻路由所用的 `204`。这是树里唯一一个调用方会据其数字
/// 行动的删除——0 意味着「本来就没东西可清」，而这条状态在历史条上呈现不同——`204`
/// 承载不了它。
pub(crate) async fn response_history_clear(
    State(s): State<Arc<WebState>>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let removed = db::with_tx(&s.pool, |c| db::response_history::clear(c, &endpoint_id))
        .map_err(map_app_error)?;
    Ok(Json(json!({ "removed": removed })))
}

#[cfg(test)]
#[path = "response_history_test.rs"]
mod tests;
