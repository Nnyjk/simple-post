//! `/api/ws/sessions`：WebSocket 会话的 REST 形态。
//!
//! 建连的响应是一条 `text/event-stream`：每帧事件按 `WsEvent` 的 JSON 形状推送。桌面端走
//! Tauri Channel，两边共用 `simple_post_core::ws` 的事件序列。
//!
//! 帧不落库：WS 是会话而不是一次请求，重连由渲染层退避，服务端不保留历史。

use std::convert::Infallible;

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::Stream;
use tokio::sync::mpsc;

use simple_post_core::ws::{self, WsEvent, WsSink};

use crate::web_api::dto::{WsCloseQuery, WsConnectBody, WsSendBody};
use crate::web_api::error::map_app_error;

/// `POST /api/ws/sessions`
pub(crate) async fn ws_connect(
    Json(body): Json<WsConnectBody>,
) -> Result<
    Sse<impl Stream<Item = Result<Event, Infallible>>>,
    crate::web_api::error::HttpErrorResponse,
> {
    if body.session_id.trim().is_empty() {
        return Err(map_app_error(
            simple_post_core::error::AppError::Validation(
                "sessionId 不能为空：关闭这条会话时要用它".into(),
            ),
        ));
    }

    let tls =
        simple_post_core::tls::resolve(&body.tls.unwrap_or_default(), body.tls_override.as_ref());
    let (tx, rx) = mpsc::unbounded_channel::<WsEvent>();
    let id = body.session_id;
    let cancel_id = id.clone();
    let sink: WsSink = Box::new(move |event| {
        // 接收端没了（浏览器断开）就结束会话：没人收的帧继续读下去只会堆积。
        if tx.send(event).is_err() {
            let _ = ws::close(&cancel_id, None, "");
            return false;
        }
        true
    });

    // `connect` 先登记写端再在后台建连，故这里返回之后首帧 `connecting` 与紧接着的
    // `send` 都不会丢。
    ws::connect(id, body.url, body.headers, body.protocols, Some(tls), sink);

    let events = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
        Some((Ok::<_, Infallible>(Event::default().data(data)), rx))
    });

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

/// `POST /api/ws/sessions/{id}/send`
pub(crate) async fn ws_send(
    Path(id): Path<String>,
    Json(body): Json<WsSendBody>,
) -> Result<StatusCode, crate::web_api::error::HttpErrorResponse> {
    ws::send(
        &id,
        &body.kind,
        body.data.as_deref(),
        body.base64.as_deref(),
    )
    .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/ws/sessions/{id}`：关闭会话，`code` / `reason` 走查询串。
pub(crate) async fn ws_close(
    Path(id): Path<String>,
    Query(query): Query<WsCloseQuery>,
) -> Result<StatusCode, crate::web_api::error::HttpErrorResponse> {
    ws::close(&id, query.code, query.reason.as_deref().unwrap_or_default())
        .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// 这三个 handler 都不碰数据库，故不取 `WebState`：会话状态住在 `simple_post_core::ws`
// 的全局注册表里。
