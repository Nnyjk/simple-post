//! `http_request_progress`：与 `http_request` 同一份参数，多的是一条进度通道。
//!
//! 请求的构造、脚本链与执行全在 core 里，与 `POST /api/http/send-progress` 调的是同一个
//! `send_with_scripts_and_progress`。这里只把传输层的进度回调转成 Channel 帧——`Channel`
//! 类型只出现在本 crate，core 与 server 只认闭包。
//!
//! **参数逐字段对应 REST body**（dual-transport invariant）：写成 `opts: …` 能编译，而每一
//! 次桌面调用都会以 `invalid args … missing required key opts` 失败。

use tauri::ipc::Channel;

use crate::domain::{EnvSnapshot, RequestSettings, TlsSettings};
use crate::dto::{HttpSendProgressEvent, SendOutcomeDto};
use crate::error::AppError;
use crate::http_executor::ProgressSink;
use crate::script::{
    send_with_scripts_and_progress, ScriptOverride, ScriptRequest, ScriptedSendInput,
};

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn http_request_progress(
    state: tauri::State<'_, crate::db::DbPool>,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    body_mode: crate::domain::BodyMode,
    timeout_ms: u64,
    follow_redirects: bool,
    max_response_size_kb: u64,
    tls: Option<TlsSettings>,
    tls_override: Option<TlsSettings>,
    endpoint_id: Option<String>,
    env: Option<EnvSnapshot>,
    pre_request_script: Option<String>,
    test_script: Option<String>,
    on_event: Channel<HttpSendProgressEvent>,
) -> Result<SendOutcomeDto, AppError> {
    // 通道关了（窗口关闭、发送被取消）时这一帧丢弃；执行本身由渲染层的 AbortSignal 通过
    // 别的路径中止，与这里无关。
    let sink: ProgressSink = Box::new(move |progress| {
        let _ = on_event.send(HttpSendProgressEvent::Progress { progress });
    });

    let outcome = send_with_scripts_and_progress(
        &state,
        ScriptedSendInput {
            endpoint_id,
            request: ScriptRequest {
                method,
                url,
                headers,
                body,
                body_mode,
            },
            env: env.unwrap_or_default(),
            settings: RequestSettings {
                timeout_ms,
                follow_redirects,
                max_response_size_kb,
                tls: tls.unwrap_or_default(),
            },
            // 端点自己的 HTTPS 设置。`None` = 继承全局默认，合成在
            // `script::send_with_scripts_and_progress` 里做，故桌面侧与 REST 侧同一个算法。
            tls_override,
            // 端点自己那两段脚本：payload 送来的那份覆盖存的那份（缺席的字段仍用存的）。
            script_override: Some(ScriptOverride {
                pre_request: pre_request_script,
                test: test_script,
            }),
        },
        Some(sink),
    )
    .await?;
    Ok(SendOutcomeDto::from(outcome))
}
