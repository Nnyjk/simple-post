//! 流式传输的 Tauri 命令：SSE（`stream_request` / `stream_cancel`）与 WebSocket
//! （含 `ws_connect` / `ws_send` / `ws_close` 三个会话命令）。
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
///
/// **必须是 `async`**（与 `stream_request` 同形）：core 的 `ws::connect` 里是裸
/// `tokio::spawn`，同步命令跑在 Tauri 的 IPC 线程上，那里没有 reactor 上下文，首连会
/// panic（"there is no reactor running"）。`async` 让 Tauri 把命令投到它自己的运行时里，
/// `spawn` 才有地方挂。`stream.rs::ws_connect_is_an_async_command` 用源码形状钉住这点。
#[tauri::command]
pub async fn ws_connect(
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
///
/// 幂等：会话可能已经自行收尾（对端关闭、建连失败、重连顶掉），此时 core 回
/// `NotFound`，这里吸收成 `Ok`。REST 的 `DELETE /api/ws/sessions/{id}` **仍然**回 404——
/// 那边有状态码可看；桌面渲染层的关闭路径是 `void handle.close()`，不 catch，未处理的
/// rejection 会当成 UI 错误冒出来。
#[tauri::command]
pub fn ws_close(
    session_id: String,
    code: Option<u16>,
    reason: Option<String>,
) -> Result<(), AppError> {
    match ws::close(&session_id, code, reason.as_deref().unwrap_or_default()) {
        Err(AppError::NotFound(_)) => Ok(()),
        other => other,
    }
}

// ----- 护栏 ---------------------------------------------------------------
//
// 这两条都是「编译得过、真机才炸」的那类回退：签名一改成同步，`cargo check` 依旧全绿，
// 只有桌面端点开 WebSocket 面板时才会 panic。故读本文件源码把形状钉住——与
// `src-tauri/src/http_request_test.rs` 同一手法。
#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = include_str!("stream.rs");

    #[test]
    fn ws_connect_is_an_async_command() {
        // 按行找声明本身，而不是在整份源码里 `contains`：本测试的断言文本也在同一个文件
        // 里，整文件搜索会把自己的字符串当证据。
        let signature = SOURCE
            .lines()
            .find(|line| line.starts_with("pub ") && line.contains(" ws_connect("))
            .expect("stream.rs 里应有 ws_connect 的声明");
        assert!(
            signature.starts_with("pub async fn "),
            "ws_connect 必须是 `pub async fn`（现在写的是 `{signature}`）：同步命令跑在 Tauri \
             的 IPC 线程上，core 里裸的 tokio::spawn 没有 reactor 上下文，桌面端首连会 panic"
        );
    }

    /// 会话已自行收尾时（注册表里没有这个 id），core 回 `NotFound`，命令必须吸收成 `Ok`。
    #[test]
    fn ws_close_absorbs_not_found() {
        let missing = "definitely-not-a-live-ws-session";
        // 先确认这个 id 确实走 NotFound 那条路，而不是被别的校验挡下。
        assert!(
            matches!(ws::close(missing, None, ""), Err(AppError::NotFound(_))),
            "缺省情况下 core 应回 NotFound"
        );
        assert!(
            ws_close(missing.to_string(), None, None).is_ok(),
            "渲染层的关闭路径不 catch，NotFound 必须被吸收"
        );
    }
}
