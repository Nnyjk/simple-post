//! 流式传输的 Tauri 命令：SSE（`stream_request` / `stream_cancel`）与 WebSocket
//! （`ws_connect` / `ws_send` / `ws_close`）。
//!
//! 事件经 `tauri::ipc::Channel` 逐条推给渲染层；`Channel` 类型只出现在本 crate，
//! core 与 server 只认事件结构本身。
//!
//! 参数逐字段对应 REST 路由（dual-transport invariant）：`stream_request` 的字段与
//! `POST /api/http/stream` 的 body 一致，`ws_connect` 与 `POST /api/ws/sessions` 一致。

use tauri::ipc::Channel;

use simple_post_core::stream::{self, StreamEvent, StreamSink};
use simple_post_core::ws::{self, WsEvent, WsSink};

use crate::domain::TlsSettings;
use crate::error::AppError;

/// 发起一次 SSE 流。命令立刻返回，事件随后到 `on_event`。
///
/// 与 `http_request` 的差别只在收尾：SSE 没有「完整响应」，故不跑请求脚本、不写
/// `response_history`。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn stream_request(
    stream_id: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    body_base64: Option<String>,
    timeout_ms: u64,
    follow_redirects: bool,
    max_response_size_kb: u64,
    tls: Option<TlsSettings>,
    tls_override: Option<TlsSettings>,
    on_event: Channel<StreamEvent>,
) -> Result<(), AppError> {
    if stream_id.trim().is_empty() {
        return Err(AppError::Validation(
            "streamId 不能为空：取消这条流时要用它".into(),
        ));
    }

    let input = crate::http_executor::HttpRequestInput {
        method,
        url,
        headers,
        body,
        body_base64,
        timeout_ms,
        follow_redirects,
        max_response_size_kb,
        // 与 REST 侧同一个合成算法：端点自己的设置整体取代全局默认。
        tls: simple_post_core::tls::resolve(&tls.unwrap_or_default(), tls_override.as_ref()),
    };
    let sink: StreamSink = Box::new(move |event| {
        // 通道关了（窗口关闭、面板卸载）时事件丢弃；上游继续读到 eof 为止，用户点「停止」
        // 会走 `stream_cancel`，那条路会立刻中止任务。
        let _ = on_event.send(event);
    });
    stream::spawn(stream_id, input, sink);
    Ok(())
}

/// 取消一次 SSE 流。幂等：id 不存在时也返回 `Ok`，渲染层的「停止」按钮不因此报错。
#[tauri::command]
pub fn stream_cancel(stream_id: String) -> Result<(), AppError> {
    let _cancelled = stream::cancel(&stream_id);
    Ok(())
}

/// 打开一条 WebSocket 会话。登记的 Channel 先于建连：首帧 `state: connecting` 不会丢。
#[tauri::command]
pub fn ws_connect(
    session_id: String,
    url: String,
    headers: Vec<(String, String)>,
    protocols: Vec<String>,
    tls: Option<TlsSettings>,
    tls_override: Option<TlsSettings>,
    on_event: Channel<WsEvent>,
) -> Result<(), AppError> {
    if session_id.trim().is_empty() {
        return Err(AppError::Validation(
            "sessionId 不能为空：关闭这条会话时要用它".into(),
        ));
    }
    let sink: WsSink = Box::new(move |event| on_event.send(event).is_ok());
    // 与 HTTP/SSE 同一套合成规则：全局设置在前，端点覆盖整体取代它。
    let tls = simple_post_core::tls::resolve(&tls.unwrap_or_default(), tls_override.as_ref());
    // `connect` 先登记写端再后台建连：返回之后首帧 `connecting` 与紧接着的 `send`
    // 都不会丢，故这里不再套一层 spawn。
    ws::connect(session_id, url, headers, protocols, Some(tls), sink);
    Ok(())
}

/// 发送一帧。`kind` 为 `text`（用 `data`）或 `binary`（用 `base64`）。
#[tauri::command]
pub fn ws_send(
    session_id: String,
    kind: String,
    data: Option<String>,
    base64: Option<String>,
) -> Result<(), AppError> {
    ws::send(&session_id, &kind, data.as_deref(), base64.as_deref())
}

/// 关闭一条会话。`code` 缺席时发一个正常关闭帧（1000）。
#[tauri::command]
pub fn ws_close(
    session_id: String,
    code: Option<u16>,
    reason: Option<String>,
) -> Result<(), AppError> {
    ws::close(&session_id, code, reason.as_deref().unwrap_or_default())
}
