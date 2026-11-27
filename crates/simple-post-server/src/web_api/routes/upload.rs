//! `/api/http/send-upload` 一族：浏览器选中的文件字节随请求一起上来。
//!
//! 桌面形态的文件分片只有本机路径，执行层按路径现读；浏览器的 `<input type=file>` 只给出
//! 一个 `File` 句柄，读不到用户磁盘上的路径。故这三条路由把文件字节放在 `multipart/form-data`
//! 里，与信封同一次请求送上来：
//!
//! - 第一个分片恒为 `request`：与 `/api/http/send` 同一份 JSON 信封，文件分片带 `size`。
//! - 其余分片按分片序号依次是各文件内容，字节原样。
//!
//! 服务端只做接力：信封先读全（它很小），随后边收边把字节转成 [`UploadFrame`] 交给执行器，
//! 文件不落盘、不整段进内存。

use std::sync::Arc;

use axum::{
    extract::{Multipart, State},
    response::Response,
    Json,
};
use tokio::sync::mpsc;

use simple_post_core::multipart::{UploadFrame, UploadReceiver};
use simple_post_core::stream;

use crate::dto::SendOutcomeDto;
use crate::web_api::dto::{HttpSendBody, HttpStreamBody};
use crate::web_api::error::map_app_error;
use crate::web_api::{HttpErrorResponse, WebState};

use super::nodes::{scripted_input, send_progress_response};
use super::stream::{sink_into, sse_response, stream_input, DOWNLINK_CAPACITY};

/// 信封与文件字节之间的有界队列。反压要一路传回浏览器：队列满时 pump 停止从 socket 读，
/// 浏览器的 `fetch` 随之减速，这段延迟只占一条队列的内存。
const UPLOAD_QUEUE: usize = 4;

/// 上传路线的请求体上限 = `/send-progress` 那个信封上限 + 文件总长上限。
///
/// 真正的语义线仍是 `MAX_REQUEST_BODY_BYTES`（装配后的总长由 core 再查一遍），这个数只决定
/// axum 会不会把合法请求拦在默认的 2 MB 上。
pub(crate) const UPLOAD_BODY_LIMIT_BYTES: usize =
    128 * 1024 * 1024 + simple_post_core::http_executor::MAX_REQUEST_BODY_BYTES;

/// `POST /api/http/send-upload`：与 `/api/http/send` 同一个响应信封。
pub(crate) async fn http_send_upload(
    State(s): State<Arc<WebState>>,
    multipart: Multipart,
) -> Result<Json<SendOutcomeDto>, HttpErrorResponse> {
    let (raw, uploads) = split_request(multipart).await.map_err(map_app_error)?;
    let body: HttpSendBody = parse_envelope(&raw).map_err(map_app_error)?;
    let mut input = scripted_input(body);
    input.uploads = Some(uploads);
    let outcome = crate::script::send_with_scripts(&s.pool, input)
        .await
        .map_err(map_app_error)?;
    Ok(Json(SendOutcomeDto::from(outcome)))
}

/// `POST /api/http/send-upload-progress`：同 `/api/http/send-progress`，文件字节从分片里来。
pub(crate) async fn http_send_upload_progress(
    State(s): State<Arc<WebState>>,
    multipart: Multipart,
) -> Result<Response, HttpErrorResponse> {
    let (raw, uploads) = split_request(multipart).await.map_err(map_app_error)?;
    let body: HttpSendBody = parse_envelope(&raw).map_err(map_app_error)?;
    let mut input = scripted_input(body);
    input.uploads = Some(uploads);
    Ok(send_progress_response(s, input))
}

/// `POST /api/http/stream-upload`：同 `/api/http/stream`，文件字节从分片里来。
pub(crate) async fn http_stream_upload(
    multipart: Multipart,
) -> Result<Response, HttpErrorResponse> {
    let (raw, uploads) = split_request(multipart).await.map_err(map_app_error)?;
    let body: HttpStreamBody = parse_envelope(&raw).map_err(map_app_error)?;
    if body.stream_id.trim().is_empty() {
        return Err(map_app_error(
            simple_post_core::error::AppError::Validation(
                "streamId 不能为空：取消这条流时要用它".into(),
            ),
        ));
    }

    let id = body.stream_id;
    let cancel_id = id.clone();
    let (tx, rx) = mpsc::channel::<stream::StreamEvent>(DOWNLINK_CAPACITY);
    stream::spawn_with_uploads(
        id,
        stream_input(body.request),
        sink_into(tx, cancel_id),
        Some(uploads),
    );
    Ok(sse_response(rx))
}

fn parse_envelope<T: serde::de::DeserializeOwned>(
    raw: &str,
) -> Result<T, simple_post_core::error::AppError> {
    serde_json::from_str(raw).map_err(|e| {
        simple_post_core::error::AppError::Validation(format!(
            "`request` 分片不是合法的 JSON 信封：{e}"
        ))
    })
}

/// 读出信封，把剩下的分片交给 pump 边收边发。
///
/// 两个阶段不能并行：pump 要独占 `Multipart`（`Field` 借它），故信封必须先整段读完。
/// 信封是原始 JSON、不含文件字节，几百 KB 以内；文件字节全在后续分片里。
async fn split_request(
    mut multipart: Multipart,
) -> Result<(String, UploadReceiver), simple_post_core::error::AppError> {
    let first = multipart
        .next_field()
        .await
        .map_err(bad_multipart)?
        .ok_or_else(|| {
            simple_post_core::error::AppError::Validation(
                "请求体是一个空的 multipart：第一个分片必须是 `request`".into(),
            )
        })?;
    if first.name() != Some("request") {
        return Err(simple_post_core::error::AppError::Validation(format!(
            "第一个分片必须叫 `request`，收到的是 {:?}",
            first.name()
        )));
    }
    let raw = first.text().await.map_err(bad_multipart)?;

    let (tx, rx) = mpsc::channel::<UploadFrame>(UPLOAD_QUEUE);
    tokio::spawn(pump(multipart, tx));
    Ok((raw, rx))
}

fn bad_multipart(e: axum::extract::multipart::MultipartError) -> simple_post_core::error::AppError {
    simple_post_core::error::AppError::Validation(format!("multipart 解析失败：{e}"))
}

/// 分片 → 字节帧。每个分片发 `Part`，逐块发 `Data`，收尾发 `End`；`End` 的个数就是执行器
/// 认得的分片序号。
///
/// 任一环节断开都只是**停下**，不补、不重试：接收端（执行器）按声明长度校验实收字节，
/// 短了就是错误，这里再写一个零长度的 `End` 只会把「传输截断」伪装成「空文件」。
///
/// 多出来的分片（信封里没声明的文件）只被丢弃：`Content-Length` 与发出的字节只由信封里的
/// `size` 决定，这些字节进不了那次请求，故它们不会破坏线格式。
async fn pump(mut multipart: Multipart, tx: mpsc::Sender<UploadFrame>) {
    loop {
        let Ok(Some(mut field)) = multipart.next_field().await else {
            return;
        };
        if tx.send(UploadFrame::Part).await.is_err() {
            return;
        }
        loop {
            match field.chunk().await {
                Ok(Some(bytes)) => {
                    if tx.send(UploadFrame::Data(bytes)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
        if tx.send(UploadFrame::End).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
#[path = "upload_test.rs"]
mod upload_tests;
