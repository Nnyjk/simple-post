//! `/api/http/stream`：SSE 流式执行。
//!
//! 请求体的字段与 `POST /api/http/send` 同形，多一个 `streamId`（取消时要用的那把钥匙）。
//! 响应是 `text/event-stream`，每条 `data: <事件 JSON>`。
//!
//! 本路由**不跑请求脚本**：测试脚本要的是完整响应，而 SSE 的响应体没有终点。前缀里的
//! `endpointId` 与脚本字段仍收下来（与 `/api/http/send` 同一份 body 形状），只是不参与执行：
//! 调用历史由渲染层在收到 `end` 时写，服务端不写——否则一次会话会出现两行。

use std::convert::Infallible;

use axum::{
    extract::Path,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::Stream;
use tokio::sync::mpsc;

use simple_post_core::http_executor::HttpRequestInput;
use simple_post_core::stream::{self, StreamEvent, StreamSink};
use simple_post_core::tls;

use crate::web_api::dto::HttpStreamBody;
use crate::web_api::error::map_app_error;

/// `POST /api/http/stream`
pub(crate) async fn http_stream(
    Json(body): Json<HttpStreamBody>,
) -> Result<
    Sse<impl Stream<Item = Result<Event, Infallible>>>,
    crate::web_api::error::HttpErrorResponse,
> {
    if body.stream_id.trim().is_empty() {
        return Err(map_app_error(
            simple_post_core::error::AppError::Validation(
                "streamId 不能为空：取消这条流时要用它".into(),
            ),
        ));
    }

    let request = body.request;
    let input = HttpRequestInput {
        method: request.method,
        url: request.url,
        headers: request.headers,
        body: request.body,
        body_base64: request.body_base64,
        timeout_ms: request.timeout_ms,
        follow_redirects: request.follow_redirects,
        max_response_size_kb: request.max_response_size_kb,
        // 与 `http_send` 同一个合成算法：端点自己的设置整体取代全局默认。
        tls: tls::resolve(
            &request.tls.unwrap_or_default(),
            request.tls_override.as_ref(),
        ),
    };

    let (tx, rx) = mpsc::unbounded_channel::<StreamEvent>();
    let id = body.stream_id;
    let cancel_id = id.clone();
    let sink: StreamSink = Box::new(move |event| {
        // 接收端没了（浏览器断开）就把任务停掉：继续读一个没人要的流没有意义。
        if tx.send(event).is_err() {
            stream::cancel(&cancel_id);
        }
    });
    stream::spawn(id, input, sink);

    let events = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
        Some((Ok::<_, Infallible>(Event::default().data(data)), rx))
    });

    // 心跳让反代与浏览器都知道连接还活着；SSE 的注释行不会被渲染层当成事件。
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

/// `DELETE /api/http/stream/{id}`：取消一条流。
///
/// 幂等：id 不存在也回 204——渲染层在「关标签页」与「主动停止」两条路上都会调它。
pub(crate) async fn http_stream_cancel(Path(id): Path<String>) -> StatusCode {
    // 取消注册表本身是幂等的：remove 不到也只说明这条流已经收尾。
    let _cancelled = stream::cancel(&id);
    StatusCode::NO_CONTENT
}
