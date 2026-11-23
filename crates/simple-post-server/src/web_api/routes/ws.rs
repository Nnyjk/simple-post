//! `/api/ws/sessions`：WebSocket 会话的 REST 形态。
//!
//! 建连的响应是一条 `text/event-stream`：每帧事件按 `WsEvent` 的 JSON 形状推送。桌面端走
//! Tauri Channel，两边共用 `simple_post_core::ws` 的事件序列。
//!
//! 帧不落库：WS 是会话而不是一次请求，重连由渲染层退避，服务端不保留历史。

use std::convert::Infallible;

use axum::{
    extract::{Path, Query},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use tokio::sync::mpsc::{self, error::TrySendError};

use simple_post_core::ws::{self, WsEvent, WsSink};

use crate::web_api::dto::{WsCloseQuery, WsConnectBody, WsSendBody};
use crate::web_api::error::map_app_error;

/// 下行队列的容量。WebSocket 是**双向**的，但会话的读循环与浏览器并不共享一个窗口：
/// 浏览器慢读时无界队列会把整个下行堆积在内存里，故取有界 + `try_send`。
const DOWNLINK_CAPACITY: usize = 256;

/// 接收端一消失就关掉 core 里的会话。
///
/// 只靠 `sink` 发送失败是不够的：下行空闲时 `sink` 一次都不会被调用，浏览器关掉标签页
/// 后服务端会话就永远留在注册表里（WS 没有读超时兜底）。`Sse` 流被 drop 时（axum 在
/// 连接断开时丢弃它），这个 guard 随 `unfold` 的 state 一起 drop，`Drop` 里发一条关闭
/// 命令把会话收尾。
struct CloseOnDrop {
    rx: mpsc::Receiver<WsEvent>,
    id: String,
}

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        // 会话可能已经自行收尾（对端关闭、建连失败、重连顶掉），`NotFound` 是正常结局。
        let _ = ws::close(&self.id, None, "");
    }
}

/// 把事件投进下行队列的那一端。
///
/// 满（浏览器慢读）与 Closed（接收端没了）都收掉会话：两种状态都救不回来，继续读只会
/// 在内存里堆事件。返回 `false` 让 core 的读循环停下并发自己的 `closed` 事件。
fn sink_into(tx: mpsc::Sender<WsEvent>, cancel_id: String) -> WsSink {
    Box::new(move |event| match tx.try_send(event) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
            let _ = ws::close(&cancel_id, None, "");
            false
        }
    })
}

/// 发起 WebSocket 会话连接：`POST /api/ws/sessions`
pub(crate) async fn ws_connect(
    Json(body): Json<WsConnectBody>,
) -> Result<Response, crate::web_api::error::HttpErrorResponse> {
    if body.session_id.trim().is_empty() {
        return Err(map_app_error(
            simple_post_core::error::AppError::Validation(
                "sessionId 不能为空：关闭这条会话时要用它".into(),
            ),
        ));
    }

    let tls =
        simple_post_core::tls::resolve(&body.tls.unwrap_or_default(), body.tls_override.as_ref());
    let (tx, rx) = mpsc::channel::<WsEvent>(DOWNLINK_CAPACITY);
    let id = body.session_id;
    let cancel_id = id.clone();
    let guard_id = id.clone();

    // `connect` 先登记写端再在后台建连，故这里返回之后首帧 `connecting` 与紧接着的
    // `send` 都不会丢。
    ws::connect(
        id,
        body.url,
        body.headers,
        body.protocols,
        Some(tls),
        sink_into(tx, cancel_id),
    );

    let state = CloseOnDrop { rx, id: guard_id };
    let events = futures_util::stream::unfold(state, |mut state| async move {
        let event = state.rx.recv().await?;
        // `WsEvent` 的字段只有 String / u64 / Option：序列化不可能失败。真失败就是代码
        // 缺陷，静默写成 `{}` 只会让渲染层丢帧且查不到原因。
        let data =
            serde_json::to_string(&event).expect("WsEvent 一定可序列化：字段只有 String/u64");
        Some((Ok::<_, Infallible>(Event::default().data(data)), state))
    });

    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response();
    // nginx 默认 `proxy_buffering on` 会把 SSE 攒成整段再发；`X-Accel-Buffering: no` 是
    // 唯一的**逐响应**关闭开关（改不了反代配置时也管用）。
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    Ok(response)
}

/// 发送 WebSocket 消息帧：`POST /api/ws/sessions/{id}/send`
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

#[cfg(test)]
#[path = "ws_test.rs"]
mod ws_tests;
