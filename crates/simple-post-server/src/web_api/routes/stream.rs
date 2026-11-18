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
    http::{HeaderName, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use tokio::sync::mpsc::{self, error::TrySendError};

use simple_post_core::http_executor::HttpRequestInput;
use simple_post_core::stream::{self, StreamEvent, StreamSink};
use simple_post_core::tls;

use crate::web_api::dto::HttpStreamBody;
use crate::web_api::error::map_app_error;

/// 下行队列的容量。有界是刻意的：SSE 是单向推送，浏览器慢读时没有任何办法把反压传回
/// 上游的读取节奏，无界队列只会把整段响应体堆在内存里。
const DOWNLINK_CAPACITY: usize = 256;

/// 把事件投进下行队列的那一端。
///
/// 通道满（浏览器慢读 / 根本不读）或接收端已消失时**直接取消**这条流：这两种状态都
/// 救不回来，继续读下去只会在内存里堆事件。代价是通道真满时 `end` 自己也可能挤不进去
/// ——刻意接受：先保证内存有界，再谈事件完整。`end` 在正常路径下（容量 256）总能送达。
fn sink_into(tx: mpsc::Sender<StreamEvent>, cancel_id: String) -> StreamSink {
    Box::new(move |event| match tx.try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
            stream::cancel(&cancel_id);
        }
    })
}

/// `POST /api/http/stream`
pub(crate) async fn http_stream(
    Json(body): Json<HttpStreamBody>,
) -> Result<Response, crate::web_api::error::HttpErrorResponse> {
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

    let (tx, rx) = mpsc::channel::<StreamEvent>(DOWNLINK_CAPACITY);
    let id = body.stream_id;
    let cancel_id = id.clone();
    stream::spawn(id, input, sink_into(tx, cancel_id));

    let events = futures_util::stream::unfold(rx, |mut rx| async move {
        let event = rx.recv().await?;
        // `StreamEvent` 的字段只有 String / u64 / Vec：序列化不可能失败。真失败就是代码
        // 缺陷，静默写成 `{}` 只会让渲染层丢帧且查不到原因。
        let data =
            serde_json::to_string(&event).expect("StreamEvent 一定可序列化：字段只有 String/u64");
        Some((Ok::<_, Infallible>(Event::default().data(data)), rx))
    });

    // 心跳让反代与浏览器都知道连接还活着；SSE 的注释行不会被渲染层当成事件。
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

/// `DELETE /api/http/stream/{id}`：取消一条流。
///
/// 幂等：id 不存在也回 204——渲染层在「关标签页」与「主动停止」两条路上都会调它。
pub(crate) async fn http_stream_cancel(Path(id): Path<String>) -> StatusCode {
    // 取消注册表本身是幂等的：remove 不到也只说明这条流已经收尾。
    let _cancelled = stream::cancel(&id);
    StatusCode::NO_CONTENT
}

#[cfg(test)]
#[path = "stream_test.rs"]
mod stream_tests;
