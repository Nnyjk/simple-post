//! SSE 流式执行。
//!
//! 请求的组装（URL、方法、请求头、body 字节）与普通发送共用
//! [`crate::http_executor::HttpRequestInput`]；出站 URL 策略、重定向逐跳检查与 HTTPS 设置也
//! 共用 `http_executor` 的构造，故两条路不会对「能连到哪、用哪份证书」各说各话。
//!
//! 本模块不跑请求脚本：测试脚本要的是完整响应，而流只有一个没有终点的响应体。渲染层也不该
//! 期待一段脚本能看见「尚未到达的最后一个事件」。
//!
//! # 事件
//!
//! 四种事件按 `type` 判别：`meta`（响应头，连接建立后一次）、`chunk`（一块原始字节的宽松
//! 解码，`seq` 从 0 递增）、`end`（正常收尾：`eof` / `stopped` / `timeout`）与 `error`
//! （连接没建起来，或读的过程中断了）。取消走 `end{reason:"stopped"}`：那是调用方主动要的
//! 结果，不是错误。
//!
//! `timeoutMs` 在这里**不是**整次请求的预算：它管建连与块间空闲。一个 SSE 连接可以开着几分
//! 钟，只要服务端持续发块。`maxResponseSizeKb` 超限时事件流以 `truncated:true` 正常收尾。
//!
//! # 取消
//!
//! 会话 id 由调用方铸造（REST 的 `streamId`、桌面端命令的 `stream_id`），本模块的注册表把 id
//! 映射到任务的取消信号；[`cancel`] 发信号，主循环在等下一块时收到它，
//! 先发 `end{reason:"stopped"}` 再关连接。**不用 `abort`**：那样任务会被就地掐断，调用方永远
//! 等不到收尾事件，面板也就停在 streaming。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::oneshot;

use crate::http_executor::{
    base_client_builder, check_url, effective_request_headers, outbound_body, parse_method,
    HttpRequestInput, OutboundPolicy, UrlVerdict,
};

/// 事件流上的一条事件。字段名是 camelCase 的线格式，渲染层与 REST 客户端读同一份。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StreamEvent {
    /// 响应头到了。`requestHeaders` 是线上那一份（含传输层补的 host / content-length）。
    Meta {
        status: u16,
        #[serde(rename = "statusText")]
        status_text: String,
        /// `[name, value]` 对，与 `HttpResponse.headers` 同形。
        headers: Vec<[String; 2]>,
        #[serde(rename = "requestHeaders")]
        request_headers: Vec<[String; 2]>,
        #[serde(rename = "contentType")]
        content_type: String,
        url: String,
    },
    /// 一块响应字节。`text` 是宽松解码的结果；`bytes` 是这一块的**原始**字节数。
    Chunk { seq: u64, text: String, bytes: u64 },
    /// 流结束。`reason` 是 `eof` / `stopped` / `timeout`。
    End {
        #[serde(rename = "durationMs")]
        duration_ms: u128,
        #[serde(rename = "sizeBytes")]
        size_bytes: u64,
        truncated: bool,
        reason: String,
    },
    /// 没建起来或中途断了。`kind` 是 `timeout` / `network`。
    Error { kind: String, message: String },
}

/// 事件出口。REST handler 把它接到 SSE 响应，桌面端接到 Tauri 的 `Channel`。
pub type StreamSink = Box<dyn FnMut(StreamEvent) + Send + 'static>;

/// 注册表里的一条会话。`token` 让收尾的任务只摘掉**自己**：同 id 重连后旧任务收尾时
/// 不能把新会话一并删掉。
struct Session {
    /// 取消信号。任务收到后发一条 `end` 再自己收尾，故这里不再留 `JoinHandle`。
    cancel: oneshot::Sender<()>,
    token: u64,
}

type Registry = Mutex<HashMap<String, Session>>;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn registry_lock() -> std::sync::MutexGuard<'static, HashMap<String, Session>> {
    // 一个 panic 在持有锁时发生，会让后续 send 永远拿不到锁——而这里存的是可以安全丢弃的
    // 会话句柄，故中毒的锁照常使用。
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// 起一个流式会话。`id` 已存在时通知旧任务收尾（旧任务会发自己的 `end{stopped}`）——调用方
/// 铸造 id，重复即意味着上一段会话作废。
pub fn spawn(id: String, input: HttpRequestInput, sink: StreamSink) {
    spawn_with_uploads(id, input, sink, None)
}

/// 同 [`spawn`]，另收这次请求上传的文件字节（Web 形态）。`uploads` 缺席时与上一条同路。
pub fn spawn_with_uploads(
    id: String,
    input: HttpRequestInput,
    sink: StreamSink,
    uploads: Option<crate::multipart::UploadReceiver>,
) {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let task_id = id.clone();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    // 先登记再起任务：`spawn` 返回之后 `cancel` / 下一次 `spawn` 立刻就能找到这一条。
    let previous = registry_lock().insert(
        id,
        Session {
            cancel: cancel_tx,
            token,
        },
    );
    tokio::spawn(async move {
        run_with_cancel_and_uploads(input, sink, Some(cancel_rx), uploads).await;
        let mut sessions = registry_lock();
        if sessions.get(&task_id).is_some_and(|s| s.token == token) {
            sessions.remove(&task_id);
        }
    });
    if let Some(previous) = previous {
        let _ = previous.cancel.send(());
    }
}

/// 取消一个会话：任务收到信号后发 `end{stopped}` 并关闭连接。id 不存在时返回 `false`。
pub fn cancel(id: &str) -> bool {
    match registry_lock().remove(id) {
        Some(session) => {
            let _ = session.cancel.send(());
            true
        }
        None => false,
    }
}

/// 跑完一次流式请求。`spawn` 在后台调它；测试可以直接 await。
pub async fn run(input: HttpRequestInput, sink: StreamSink) {
    run_with_cancel(input, sink, None).await;
}

/// 与 [`run`] 相同，另收一个取消信号：收到后发 `end{reason:"stopped"}` 再返回。
pub async fn run_with_cancel(
    input: HttpRequestInput,
    sink: StreamSink,
    cancel: Option<oneshot::Receiver<()>>,
) {
    run_with_cancel_and_uploads(input, sink, cancel, None).await
}

/// 同 [`run_with_cancel`]，另收这次请求上传的文件字节（Web 形态）。`uploads` 缺席时与
/// 上一条逐字节同路。
pub async fn run_with_cancel_and_uploads(
    input: HttpRequestInput,
    mut sink: StreamSink,
    mut cancel: Option<oneshot::Receiver<()>>,
    uploads: Option<crate::multipart::UploadReceiver>,
) {
    let start = Instant::now();

    // 取消信号在整个生命周期里都有效：等响应头、等下一块都同时盯着它。
    let cancelled = async {
        match cancel.as_mut() {
            Some(rx) => {
                let _ = rx.await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(cancelled);

    if let UrlVerdict::Refused(reason) = check_url(&input.url, OutboundPolicy::from_env()) {
        sink(StreamEvent::Error {
            kind: "network".into(),
            message: reason,
        });
        return;
    }

    let timeout = (input.timeout_ms > 0).then(|| Duration::from_millis(input.timeout_ms));

    let mut builder = match base_client_builder(&input, OutboundPolicy::from_env()) {
        Ok(b) => b,
        Err(message) => {
            sink(StreamEvent::Error {
                kind: "network".into(),
                message,
            });
            return;
        }
    };
    if let Some(t) = timeout {
        builder = builder.connect_timeout(t);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => {
            sink(StreamEvent::Error {
                kind: "network".into(),
                message: format!("client build: {e}"),
            });
            return;
        }
    };

    let (outbound, multipart_ct) = match outbound_body(&input, uploads).await {
        Ok(Some((body, ct))) => (Some(body), ct),
        Ok(None) => (None, None),
        Err(message) => {
            sink(StreamEvent::Error {
                kind: "network".into(),
                message,
            });
            return;
        }
    };

    let mut headers = input.headers.clone();
    if let Some(ct) = multipart_ct.as_deref() {
        crate::multipart::set_content_type(&mut headers, ct);
    }

    let mut request = client.request(parse_method(&input.method), &input.url);
    for (k, v) in &headers {
        if !k.is_empty() {
            request = request.header(k, v);
        }
    }
    let body_len = outbound.as_ref().map(|b| b.len());
    if let Some(body) = outbound {
        // 文件段边读边发：流式 body 没有 `size_hint`，长度要自己写；调用方写过同名头时
        // 不再追加第二条。
        if !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            request = request.header(reqwest::header::CONTENT_LENGTH, body.len().to_string());
        }
        request = match body.as_inline() {
            Some(bytes) => request.body(bytes.to_vec()),
            None => request.body(reqwest::Body::wrap_stream(
                body.chunks(crate::multipart::CHUNK_BYTES),
            )),
        };
    }

    let response = tokio::select! {
        _ = &mut cancelled => {
            // 还没拿到响应头就被取消：仍然给出这条约定好的收尾事件。
            emit_end(&mut sink, start, 0, false, "stopped");
            return;
        }
        sent = async {
            match timeout {
                // 建连（或等响应头）超了 `timeoutMs`：与块间空闲同一档收尾。
                Some(t) => tokio::time::timeout(t, request.send()).await.ok(),
                None => Some(request.send().await),
            }
        } => match sent {
            Some(Ok(r)) => r,
            Some(Err(e)) if e.is_timeout() => {
                emit_end(&mut sink, start, 0, false, "timeout");
                return;
            }
            Some(Err(e)) => {
                sink(StreamEvent::Error {
                    kind: "network".into(),
                    message: crate::http_executor::describe_reqwest_error(&e),
                });
                return;
            }
            None => {
                emit_end(&mut sink, start, 0, false, "timeout");
                return;
            }
        },
    };

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let header_pairs: Vec<[String; 2]> = response
        .headers()
        .iter()
        .map(|(k, v)| [k.to_string(), v.to_str().unwrap_or("").to_string()])
        .collect();

    sink(StreamEvent::Meta {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers: header_pairs,
        request_headers: effective_request_headers(&input.url, &headers, body_len),
        content_type,
        url: response.url().to_string(),
    });

    let max_bytes = input.max_response_size_kb.saturating_mul(1024);
    let mut response = response;
    let mut seq: u64 = 0;
    // 已收到的字节：体积上限与 `end.sizeBytes` 都按它算（文本解码可能比字节短或长）。
    let mut size: u64 = 0;
    // 上一块末尾留下的、还凑不成一个字符的字节。SSE 的块边界由网络决定，多字节字符可能被
    // 切在两块之间；逐块 lossy 会把它变成两个 U+FFFD。
    let mut pending: Vec<u8> = Vec::new();

    loop {
        if max_bytes > 0 && size >= max_bytes {
            // 截断点已到：残留的半截字符不再交出去。
            flush_pending(&mut sink, &mut pending, &mut seq, false);
            emit_end(&mut sink, start, size, true, "eof");
            return;
        }

        let next = tokio::select! {
            _ = &mut cancelled => {
                flush_pending(&mut sink, &mut pending, &mut seq, true);
                emit_end(&mut sink, start, size, false, "stopped");
                return;
            }
            next = async {
                match timeout {
                    Some(t) => match tokio::time::timeout(t, response.chunk()).await {
                        Ok(chunk) => Some(chunk.map_err(|e| e.to_string())),
                        // 块间空闲超了：正常收尾的一档，不是 `error`。
                        Err(_) => None,
                    },
                    None => Some(response.chunk().await.map_err(|e| e.to_string())),
                }
            } => next,
        };

        let Some(next) = next else {
            flush_pending(&mut sink, &mut pending, &mut seq, true);
            emit_end(&mut sink, start, size, false, "timeout");
            return;
        };

        let chunk = match next {
            Ok(Some(chunk)) => chunk,
            Ok(None) => {
                flush_pending(&mut sink, &mut pending, &mut seq, true);
                emit_end(&mut sink, start, size, false, "eof");
                return;
            }
            Err(message) => {
                sink(StreamEvent::Error {
                    kind: "network".into(),
                    message,
                });
                return;
            }
        };

        // 上限落在这一块的中间时只收到上限为止，然后按正常收尾报 truncated。
        let (bytes, truncated) = match max_bytes {
            0 => (chunk.to_vec(), false),
            cap if size + chunk.len() as u64 >= cap => {
                let take = (cap - size) as usize;
                (chunk[..take].to_vec(), true)
            }
            _ => (chunk.to_vec(), false),
        };
        size += bytes.len() as u64;
        pending.extend_from_slice(&bytes);

        // 上限切在多字节字符中间时丢掉半个字符，而不是吐一个替换字符：截断的残留直接丢，
        // 故这里传 false（`flush_pending` 的文档就是这么约定的）。
        flush_pending(&mut sink, &mut pending, &mut seq, false);

        if truncated {
            emit_end(&mut sink, start, size, true, "eof");
            return;
        }
    }
}

/// 把 `pending` 里能完整解码的前缀作为一条 `chunk` 发出去。
///
/// `flush_residual` 只在正常收尾时为真：那时末尾凑不成字符的字节按 lossy 交出去（上游给了
/// 非法字节，替换字符是唯一诚实的表达）。体积上限截断则传 false，残留直接丢掉，不吐半截。
fn flush_pending(
    sink: &mut StreamSink,
    pending: &mut Vec<u8>,
    seq: &mut u64,
    flush_residual: bool,
) {
    if pending.is_empty() {
        return;
    }
    let before = pending.len();
    let mut text = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                text.push_str(s);
                pending.clear();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                // 安全：`valid_up_to` 之前一定是合法 UTF-8。
                text.push_str(std::str::from_utf8(&pending[..valid]).unwrap_or_default());
                match e.error_len() {
                    Some(n) => {
                        text.push('\u{FFFD}');
                        pending.drain(..valid + n);
                    }
                    None => {
                        pending.drain(..valid);
                        break;
                    }
                }
            }
        }
    }
    if flush_residual && !pending.is_empty() {
        text.push_str(&String::from_utf8_lossy(pending));
        pending.clear();
    }
    let consumed = before - pending.len();
    if text.is_empty() && consumed == 0 {
        return;
    }
    *seq += 1;
    sink(StreamEvent::Chunk {
        seq: *seq - 1,
        text,
        bytes: consumed as u64,
    });
}

fn emit_end(sink: &mut StreamSink, start: Instant, size: u64, truncated: bool, reason: &str) {
    sink(StreamEvent::End {
        duration_ms: start.elapsed().as_millis(),
        size_bytes: size,
        truncated,
        reason: reason.to_string(),
    });
}

#[cfg(test)]
#[path = "stream_test.rs"]
mod tests;
