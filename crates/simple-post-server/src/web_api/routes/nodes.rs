//! 合并后的资源树的 REST handler。
//!
//! 模块 / 集合 / endpoint 统一在一张 `nodes` 表里，故这些 handler 也不分资源类型。
//! 两个只属于 endpoint 的子资源（`/duplicate`、`/send`）保留，只是改挂在节点之下：
//! 它们是对单个请求的动作，不是树结构的一部分。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput, HttpResult};

use super::super::{ensure_server_id, map_app_error, HttpErrorResponse, WebState};

use super::super::dto::{HttpSendBody, NodeCreateBody, NodeMoveBody};

/// `POST /api/http/send` —— 裸 URL 逃生口。
///
/// **这是本 crate 唯一对外发请求的路由**：URL 完全由 body 决定，绕过端点配置的
/// base URL 解析，出站策略只在 `http_executor` 执行层把关。节点级的 `/send` 不再
/// 存在，否则就是第二条不受管辖的出站通道——`base_url_defs_test` 盯住它不可路由。
pub(crate) async fn http_send(
    State(_s): State<Arc<WebState>>,
    Json(body): Json<HttpSendBody>,
) -> Result<Json<HttpResult>, HttpErrorResponse> {
    let input = HttpRequestInput {
        method: body.method,
        url: body.url,
        headers: body.headers,
        body: body.body,
        timeout_ms: body.timeout_ms,
        follow_redirects: body.follow_redirects,
        max_response_size_kb: body.max_response_size_kb,
    };
    // 返回值是 `HttpResult`（成功或传输层失败都包在 `ok=false` 里），两者都能干净地
    // 序列化成 JSON。
    Ok(Json(http_executor::execute(&input).await))
}

/// `GET /api/nodes`
///
/// `rootId` 把结果收窄到一个项目——SPA 总有且只有一个当前项目，故这一个参数就能覆盖
/// 按项目/按类型/搜索三种列表。`type` 与 `q` 是可选过滤；`q` 搜端点的 name、url 与
/// description，故它要进 `endpoint_details` 而不是只匹配节点。
pub(crate) async fn nodes_list(
    State(s): State<Arc<WebState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = match params.get("q") {
        Some(q) => db::with_conn(&s.pool, |c| db::nodes::search(c, q)).map_err(map_app_error)?,
        None => match params.get("type") {
            Some(t) => {
                let kind = ResourceNodeType::parse(t).ok_or_else(|| {
                    map_app_error(AppError::Validation(format!("unknown node type {t:?}")))
                })?;
                let root_id = params.get("rootId").map(String::as_str).ok_or_else(|| {
                    map_app_error(AppError::Validation("type filter requires rootId".into()))
                })?;
                db::with_conn(&s.pool, |c| {
                    db::nodes::list_by_root_and_type(c, root_id, kind)
                })
                .map_err(map_app_error)?
            }
            None => match params.get("rootId") {
                Some(root_id) => db::with_conn(&s.pool, |c| db::nodes::list_by_root(c, root_id))
                    .map_err(map_app_error)?,
                // 首屏还没有当前项目时返回整棵树，好让 SPA 仍能渲染出点什么。
                None => db::with_conn(&s.pool, db::nodes::list_all).map_err(map_app_error)?,
            },
        },
    };
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

/// `GET /api/nodes/:id`
pub(crate) async fn nodes_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let row = db::with_conn(&s.pool, |c| db::nodes::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

/// `POST /api/nodes`
///
/// endpoint 以一次 body 带来两半：结构节点与它的 `detail`。两者在一个事务里写入，
/// `db::nodes::create` 拒绝不匹配的配对——endpoint 没有 payload、或集合带着
/// payload，都不行——而不是留下一棵读者打不开的树。
pub(crate) async fn nodes_create(
    State(s): State<Arc<WebState>>,
    Json(mut body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    ensure_server_id(&mut body);
    let parsed: NodeCreateBody = serde_json::from_value(body)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;

    let node = parsed.node;
    // 子节点的归属由父节点蕴含。故在此推导，调用方只需点名父节点，两者不可能矛盾——
    // 规则与理由都和 Tauri 命令里 root_id 为空的分支相同。
    let mut root_id = node.root_id.clone().unwrap_or_default();
    if root_id.is_empty() {
        if let Some(parent) = &node.parent_id {
            root_id = db::with_conn(&s.pool, |c| db::nodes::root_id_of(c, parent))
                .map_err(map_app_error)?;
        }
    }
    let node = node.into_node(&root_id);

    // 在此展开可选的 detail 块而不是在 DTO 里，好让半填的 endpoint（"新建请求"只发
    // 一个 url）采用与 Tauri 命令相同的默认值。
    let detail = parsed.detail.map(|d| d.into_detail(&node.id));

    let created = db::with_tx(&s.pool, |c| db::nodes::create(c, &node, detail.as_ref()))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}

/// `PATCH /api/nodes/:id` —— 只改结构列。
///
/// endpoint 配置另有路由。混在一起会让一次改名去重写根本没变的 JSON 列。
pub(crate) async fn nodes_update(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::ResourceNodePatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // `with_tx` 要 `Fn`，故 patch 克隆而非移动。
    let row = db::with_tx(&s.pool, |c| db::nodes::update(c, &id, patch.clone()))
        .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&row).unwrap_or(Value::Null)))
}

/// `DELETE /api/nodes/:id`
///
/// 连带整棵子树：`nodes.parent_id` 自引用且 `ON DELETE CASCADE`，一条语句即可拆完。
pub(crate) async fn nodes_delete(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| db::nodes::delete(c, &id)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/nodes/move`
///
/// `parentId` 为 null 表示「项目顶层」，且仅对模块合法。目的地位于哪张表由父节点的
/// `type` 回答，不再需要单独的 `parentKind`。
pub(crate) async fn nodes_move(
    State(s): State<Arc<WebState>>,
    Json(body): Json<NodeMoveBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    db::with_tx(&s.pool, |c| {
        db::nodes::move_many(c, &body.node_ids, body.parent_id.as_deref(), body.index)
    })
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- 只属于 endpoint 的子资源 ---------------------------------------

/// `GET /api/endpoint-details` —— 所有 endpoint 的配置，打平。
///
/// renderer 侧与 `nodes_list` 配对：树读结构、HTTP 测试器读配置，两者差异大到每次
/// 启动都 join 会为一份树从不绘制的数据付费。客户端按 `nodeId` 建键。
pub(crate) async fn endpoint_details_list(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let projects = db::with_conn(&s.pool, db::projects::list).map_err(map_app_error)?;
    let mut out = Vec::new();
    for p in projects {
        out.extend(
            db::with_conn(&s.pool, |c| db::endpoint_details::list_by_root(c, &p.id))
                .map_err(map_app_error)?,
        );
    }
    Ok(Json(
        serde_json::to_value(&out).unwrap_or_else(|_| json!([])),
    ))
}

/// `GET /api/nodes/:id/endpoint-detail`
pub(crate) async fn node_endpoint_detail_get(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let d = db::with_conn(&s.pool, |c| db::endpoint_details::get(c, &id)).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&d).unwrap_or(Value::Null)))
}

/// `PATCH /api/nodes/:id/endpoint-detail`
pub(crate) async fn node_endpoint_detail_patch(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    Json(patch): Json<crate::domain::EndpointDetailPatch>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let d = db::with_tx(&s.pool, |c| {
        db::endpoint_details::update(c, &id, patch.clone())
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&d).unwrap_or(Value::Null)))
}

/// `POST /api/nodes/:id/duplicate` 的 body。
///
/// 整块可选——这个 body 的全部意义就是那个 id，而没有本地副本需要保持一致的调用方
/// （脚本、curl）可以直接省掉，让服务端铸一个。定义在此而非 `dto.rs`，好让整条
/// 路由的契约与强制它的 handler 待在一起。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeDuplicateBody {
    /// 副本要写入的 id，原样采纳。
    ///
    /// 用 `#[serde(default)]` 而非裸的 `Option<String>`，因为浏览器在没有 id 可给时
    /// 会发 `null`：「缺失」与「显式 null」都表示*在此铸一个*，SPA 发 null 不该变成
    /// 400。
    #[serde(default)]
    pub new_id: Option<String>,
}

/// `POST /api/nodes/{id}/duplicate` —— 仅限 endpoint。
///
/// 委派给 `db::nodes::duplicate`，后者在一个事务里复制节点及其 detail，并在一开始
/// 就拒绝非 endpoint——而不是造出一个看着像半成品副本的空集合。
pub(crate) async fn node_duplicate(
    State(s): State<Arc<WebState>>,
    Path(id): Path<String>,
    // 用 `Option<Json<_>>` 而非 `Json<_>`：body 确实是可选的，必填 extractor 会让
    // 所有写在 SPA 开始发 id 之前的调用方都吃 400。axum 0.8 起 `Option<T>` 只吞
    // 「字段缺失」，畸形 JSON 仍会走 rejection —— 正好是这里要的分界。
    body: Option<Json<NodeDuplicateBody>>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let new_id = body.and_then(|Json(b)| b.new_id);
    let created = db::with_tx(&s.pool, |c| db::nodes::duplicate(c, &id, new_id.as_deref()))
        .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&created).unwrap_or(Value::Null)),
    ))
}
