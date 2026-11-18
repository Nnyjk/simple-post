//! WebSocket 会话：帧的收发与事件流。
//!
//! 会话与 HTTP 请求是两条路：WS 没有「一次请求/一次响应」的边界，帧也不进 `response_history`。
//! `connect` 立刻返回，建连、读写、关闭都在后台任务里发生，事件按 `WsEvent` 推给出口。
//!
//! # TLS
//!
//! WSS 与 HTTPS 用同一份 `TlsSettings`（`core::tls::resolve` 合成的全局默认 + 端点覆盖）：
//! 三项都没配时用内置的 Mozilla 根证书集；`ca_certificate_path` 里的证书**追加**到内置根
//! 之后（不是替换）；客户端证书与私钥一起给出时用于 mTLS；`skip_verify` 打开时信任链不
//! 参与，校验器全部放行。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::Connector;

use crate::domain::TlsSettings;
use crate::error::AppError;
use crate::http_executor::{check_url, OutboundPolicy, UrlVerdict};

/// 建连超时。与 `http_executor` 的总超时不是一回事：WS 建连之后要一直开着。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 一帧的去向或一条控制指令。
enum Command {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>, String),
}

/// 事件出口。返回 `false` 表示接收端已经不要了（浏览器断开、Channel 关闭）。
pub type WsSink = Box<dyn FnMut(WsEvent) -> bool + Send + 'static>;

/// 会话注册表：id 由调用方铸造，`send` / `close` 靠它找到写端。
static SESSIONS: OnceLock<Mutex<HashMap<String, mpsc::UnboundedSender<Command>>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, mpsc::UnboundedSender<Command>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 中毒的锁照样能读能写：里面只是一个 `HashMap`，没有会被打断的复合不变量。
fn sessions_lock() -> std::sync::MutexGuard<'static, HashMap<String, mpsc::UnboundedSender<Command>>>
{
    sessions().lock().unwrap_or_else(|e| e.into_inner())
}

/// 一条事件。`type` 判别四种形状，字段名与 `src/lib/stream-transport/types.ts` 逐字一致。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WsEvent {
    /// 连接状态。只有 `connecting` 与 `open`；断开是单独的 `closed`。
    State { state: String },
    /// 一帧。文本在 `data`，二进制在 `base64`（`bytes` 是原始字节数）。
    Frame {
        direction: String,
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        base64: Option<String>,
        bytes: u64,
        at: u64,
    },
    /// 连接结束。没有收到关闭帧时 `code` 为 `None`。
    Closed {
        code: Option<u16>,
        reason: String,
        at: u64,
    },
    /// 建连或读写失败。
    Error { kind: String, message: String },
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 发起一条会话。函数返回时写端已登记，故紧接着的 `send` 不会丢。
///
/// TLS 由调用方用 `tls::resolve` 合成后传来：`skip_verify` 会整份替换校验器（此时内置根
/// 不参与）；否则内置 Mozilla 根在前、`ca_certificate_path` 的证书追加在后；客户端证书
/// 与私钥必须成对。错误都指到具体文件路径。
pub fn connect(
    id: String,
    url: String,
    headers: Vec<(String, String)>,
    protocols: Vec<String>,
    tls: Option<TlsSettings>,
    sink: WsSink,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let own = tx.clone();
    // 同 id 的旧会话先让位：渲染层重连时沿用同一个 id。旧任务自己握着 `own` 一份强引用，
    // 光把注册表换成新的不会让它的 `recv()` 变成 `None`，故必须显式投一条 `Close`——旧任务
    // 收到后发关闭帧、报一条 `Closed` 再收尾。它的 `finish` 有 `same_channel` 兜着，摘不掉
    // 刚登记的这条。
    if let Some(previous) = sessions_lock().insert(id.clone(), tx) {
        let _ = previous.send(Command::Close(
            Some(1000),
            "会话已被同一 id 的新连接取代".into(),
        ));
    }
    tokio::spawn(run_session(
        SessionSpec {
            id,
            url,
            headers,
            protocols,
            tls,
        },
        sink,
        rx,
        own,
    ));
}

/// 发送一帧。会话不存在时报 `NotFound`，让调用方知道连接已经不在了。
pub fn send(
    id: &str,
    kind: &str,
    data: Option<&str>,
    base64: Option<&str>,
) -> Result<(), AppError> {
    let command = match kind {
        "text" => {
            // 文本帧只能走 `data`：给了 `base64` 说明调用方把两种载荷混在一起了，静默发一个
            // 空文本帧会让面板显示「发出去了」而服务端什么都没收到。
            if base64.is_some_and(|s| !s.trim().is_empty()) {
                return Err(AppError::Validation(
                    "kind=text 时只给 data；base64 是二进制帧的字段".into(),
                ));
            }
            Command::Text(data.unwrap_or_default().to_string())
        }
        "binary" => {
            let raw = base64
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::Validation("kind=binary 时必须给 base64".into()))?;
            Command::Binary(
                base64::engine::general_purpose::STANDARD
                    .decode(raw.as_bytes())
                    .map_err(|e| AppError::Validation(format!("base64 解码失败：{e}")))?,
            )
        }
        other => {
            return Err(AppError::Validation(format!(
                "unknown frame kind {other:?}; expected \"text\" or \"binary\""
            )))
        }
    };
    let sender = sessions_lock()
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("websocket session {id}")))?;
    sender
        .send(command)
        .map_err(|_| AppError::NotFound(format!("websocket session {id}")))
}

/// 关闭一条会话。`code` 缺席时发一个正常的空关闭帧。
pub fn close(id: &str, code: Option<u16>, reason: &str) -> Result<(), AppError> {
    let sender = sessions_lock()
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("websocket session {id}")))?;
    sender
        .send(Command::Close(code, reason.to_string()))
        .map_err(|_| AppError::NotFound(format!("websocket session {id}")))
}

fn emit(sink: &mut WsSink, event: WsEvent) -> bool {
    sink(event)
}

/// 一次建连要带的东西。参数多了之后单独成结构，`run_session` 才好读。
struct SessionSpec {
    id: String,
    url: String,
    headers: Vec<(String, String)>,
    protocols: Vec<String>,
    tls: Option<TlsSettings>,
}

async fn run_session(
    spec: SessionSpec,
    mut sink: WsSink,
    mut commands: mpsc::UnboundedReceiver<Command>,
    own: mpsc::UnboundedSender<Command>,
) {
    let SessionSpec {
        id,
        url,
        headers,
        protocols,
        tls,
    } = spec;
    if !emit(
        &mut sink,
        WsEvent::State {
            state: "connecting".into(),
        },
    ) {
        finish(&id, &own);
        return;
    }

    // 与 SSE 走同一条出站策略：WS 也不能被当成打到内网（含 169.254.169.254 的云元数据）的
    // 通道。必须赶在拨号之前；此时 `connecting` 已经报过，故这里只报错、不补 `closed`。
    if let UrlVerdict::Refused(reason) = check_ws_url(&url, OutboundPolicy::from_env()) {
        emit(
            &mut sink,
            WsEvent::Error {
                kind: "protocol".into(),
                message: reason,
            },
        );
        finish(&id, &own);
        return;
    }

    let request = match build_request(&url, &headers, &protocols) {
        Ok(request) => request,
        Err(message) => {
            emit(
                &mut sink,
                WsEvent::Error {
                    kind: "protocol".into(),
                    message: message.to_string(),
                },
            );
            finish(&id, &own);
            return;
        }
    };

    let tls = tls.unwrap_or_default();
    let connector = match ws_connector(&tls) {
        Ok(connector) => connector,
        Err(message) => {
            emit(
                &mut sink,
                WsEvent::Error {
                    kind: "protocol".into(),
                    message: message.to_string(),
                },
            );
            finish(&id, &own);
            return;
        }
    };

    let handshake = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_tls_with_config(
            request,
            Some(WebSocketConfig::default()),
            false,
            connector,
        ),
    )
    .await;

    let ws = match handshake {
        Err(_) => {
            emit(
                &mut sink,
                WsEvent::Error {
                    kind: "timeout".into(),
                    message: format!("WebSocket 建连超过 {} 秒", CONNECT_TIMEOUT.as_secs()),
                },
            );
            finish(&id, &own);
            return;
        }
        Ok(Err(e)) => {
            emit(
                &mut sink,
                WsEvent::Error {
                    kind: "network".into(),
                    message: e.to_string(),
                },
            );
            finish(&id, &own);
            return;
        }
        Ok(Ok((ws, _response))) => ws,
    };

    if !emit(
        &mut sink,
        WsEvent::State {
            state: "open".into(),
        },
    ) {
        finish(&id, &own);
        return;
    }

    let (mut write, mut read) = ws.split();
    let mut close_event: Option<WsEvent> = None;

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    Command::Text(text) => {
                        if write.send(Message::Text(text.clone().into())).await.is_err() {
                            break;
                        }
                        let bytes = text.len() as u64;
                        if !emit(&mut sink, WsEvent::Frame {
                            direction: "out".into(),
                            kind: "text".into(),
                            data: Some(text),
                            base64: None,
                            bytes,
                            at: now_ms(),
                        }) {
                            break;
                        }
                    }
                    Command::Binary(bytes) => {
                        if write.send(Message::Binary(bytes.clone().into())).await.is_err() {
                            break;
                        }
                        let size = bytes.len() as u64;
                        if !emit(&mut sink, WsEvent::Frame {
                            direction: "out".into(),
                            kind: "binary".into(),
                            data: None,
                            base64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                            bytes: size,
                            at: now_ms(),
                        }) {
                            break;
                        }
                    }
                    Command::Close(code, reason) => {
                        let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(code.unwrap_or(1000)),
                            reason: reason.clone().into(),
                        };
                        let _ = write.send(Message::Close(Some(frame))).await;
                        close_event = Some(WsEvent::Closed { code: Some(code.unwrap_or(1000)), reason, at: now_ms() });
                        break;
                    }
                }
            }
            message = read.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let bytes = text.len() as u64;
                        if !emit(&mut sink, WsEvent::Frame {
                            direction: "in".into(),
                            kind: "text".into(),
                            data: Some(text.to_string()),
                            base64: None,
                            bytes,
                            at: now_ms(),
                        }) {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        let size = bytes.len() as u64;
                        if !emit(&mut sink, WsEvent::Frame {
                            direction: "in".into(),
                            kind: "binary".into(),
                            data: None,
                            base64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                            bytes: size,
                            at: now_ms(),
                        }) {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason) = match frame {
                            Some(frame) => (Some(u16::from(frame.code)), frame.reason.to_string()),
                            None => (None, String::new()),
                        };
                        close_event = Some(WsEvent::Closed { code, reason, at: now_ms() });
                        break;
                    }
                    // Ping 要回 Pong；Pong 与其它协议消息对渲染层没有意义。
                    Some(Ok(Message::Ping(payload))) => {
                        if write.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    // 读侧断了：`Error` 已经让面板显示「连接失败」，再补一条 `Closed` 会被
                    // `wsClosed` 覆盖成「未连接」，与建连失败的显示不一致。故就地收尾。
                    Some(Err(e)) => {
                        emit(&mut sink, WsEvent::Error { kind: "network".into(), message: e.to_string() });
                        finish(&id, &own);
                        return;
                    }
                    None => break,
                }
            }
        }
    }

    // 走到这里只有正常收尾：对端发了 `Close`（已经备好事件）、eof，或写端坏了。读侧错误在
    // 上面就地 return 了，不会再被这条兜底盖掉。
    emit(
        &mut sink,
        close_event.unwrap_or(WsEvent::Closed {
            code: None,
            reason: String::new(),
            at: now_ms(),
        }),
    );
    finish(&id, &own);
}

/// 从注册表摘掉自己。只有仍是自己那条时才移除，避免把重连后的新会话删掉。
fn finish(id: &str, own: &mpsc::UnboundedSender<Command>) {
    let mut map = sessions_lock();
    // 只有仍是自己那条时才移除：重连沿用同一个 id，旧任务的收尾不能把新会话删掉。
    if map.get(id).is_some_and(|current| current.same_channel(own)) {
        map.remove(id);
    }
}

/// 与 SSE / 普通发送同一条出站判定，只是 WS 的 scheme 不同。
///
/// [`check_url`] 的入口是 HTTP，只认 `http`/`https`；换成等价的那一对再过它，两条路对
/// 「能连到哪」就不会各说各话。拒绝理由里再换回调用方给的 URL，面板显示的是他真正填的那条。
fn check_ws_url(url: &str, policy: OutboundPolicy) -> UrlVerdict {
    let probe = http_probe(url);
    match check_url(&probe, policy) {
        UrlVerdict::Refused(reason) => UrlVerdict::Refused(reason.replace(&probe, url)),
        UrlVerdict::Allowed => UrlVerdict::Allowed,
    }
}

/// 把 `ws`/`wss` 换成 `http`/`https`，其余原样。scheme 大小写不敏感（RFC 3986 3.1），故按
/// 小写形态认前缀，切片仍用原串——前缀是 ASCII，长度不受大小写影响。
fn http_probe(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("ws://") {
        format!("http://{}", &url["ws://".len()..])
    } else if lower.starts_with("wss://") {
        format!("https://{}", &url["wss://".len()..])
    } else {
        url.to_string()
    }
}

fn build_request(
    url: &str,
    headers: &[(String, String)],
    protocols: &[String],
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("WebSocket URL 无效：{e}"))?;
    {
        let map = request.headers_mut();
        for (name, value) in headers {
            let name = tokio_tungstenite::tungstenite::http::HeaderName::try_from(name.as_str())
                .map_err(|e| format!("WebSocket header 名无效（{name}）：{e}"))?;
            let value = tokio_tungstenite::tungstenite::http::HeaderValue::try_from(value.as_str())
                .map_err(|e| format!("WebSocket header 值无效（{name}）：{e}"))?;
            map.append(name, value);
        }
        if !protocols.is_empty() {
            let joined = protocols.join(", ");
            let value = tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&joined)
                .map_err(|e| format!("子协议无效：{e}"))?;
            map.append(
                tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL,
                value,
            );
        }
    }
    Ok(request)
}

/// `None` = 用 `tokio-tungstenite` 内置的 Mozilla 根证书。
///
/// 只有三项都没配且没开 `skip_verify` 才返回 `None`；其余情况自建一份 rustls 配置，
/// 根的语义与 `tls::apply_to_builder` 相同：内置根在前，自定义 CA 追加在后。
fn ws_connector(tls: &TlsSettings) -> Result<Option<Connector>, AppError> {
    let ca = crate::tls::non_empty(&tls.ca_certificate_path);
    let cert = crate::tls::non_empty(&tls.client_certificate_path);
    let key = crate::tls::non_empty(&tls.client_key_path);
    if !tls.skip_verify && ca.is_none() && cert.is_none() && key.is_none() {
        return Ok(None);
    }

    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| AppError::Validation(format!("WS 的 rustls 无法启用默认协议版本: {e}")))?;

    let builder = if tls.skip_verify {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAny))
    } else {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(path) = ca {
            let pem = crate::tls::normalise_pem_boundaries(&crate::tls::read_pem(path, "CA 证书")?);
            let certs = rustls_pemfile::certs(&mut std::io::Cursor::new(&pem))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Validation(format!("CA 证书文件 {path} 解析失败: {e}")))?;
            if certs.is_empty() {
                return Err(AppError::Validation(format!(
                    "CA 证书文件 {path} 里没有可用的证书"
                )));
            }
            for cert in &certs {
                roots.add(cert.clone()).map_err(|e| {
                    AppError::Validation(format!("CA 证书文件 {path} 里的证书无法加入信任库: {e}"))
                })?;
            }
        }
        builder.with_root_certificates(roots)
    };

    let config = match (cert, key) {
        (Some(cert_path), Some(key_path)) => {
            let cert_pem = crate::tls::normalise_pem_boundaries(&crate::tls::read_pem(
                cert_path,
                "客户端证书",
            )?);
            let certs = rustls_pemfile::certs(&mut std::io::Cursor::new(&cert_pem))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    AppError::Validation(format!("客户端证书文件 {cert_path} 解析失败: {e}"))
                })?;
            if certs.is_empty() {
                return Err(AppError::Validation(format!(
                    "客户端证书文件 {cert_path} 里没有可用的证书"
                )));
            }
            let key_pem = crate::tls::normalise_pem_boundaries(&crate::tls::read_pem(
                key_path,
                "客户端私钥",
            )?);
            let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(&key_pem))
                .map_err(|e| {
                    AppError::Validation(format!("客户端私钥文件 {key_path} 解析失败: {e}"))
                })?
                .ok_or_else(|| {
                    AppError::Validation(format!("客户端私钥文件 {key_path} 里没有私钥"))
                })?;
            builder.with_client_auth_cert(certs, key).map_err(|e| {
                AppError::Validation(format!(
                    "客户端证书与私钥无法组成一对（{cert_path} 与 {key_path}）: {e}"
                ))
            })?
        }
        (Some(cert_path), None) => {
            return Err(AppError::Validation(format!(
                "填了客户端证书路径 {cert_path} 却没有客户端私钥路径：证书要连着私钥一起出示"
            )));
        }
        (None, Some(key_path)) => {
            return Err(AppError::Validation(format!(
                "填了客户端私钥路径 {key_path} 却没有客户端证书路径：证书要连着私钥一起出示"
            )));
        }
        (None, None) => builder.with_no_client_auth(),
    };
    Ok(Some(Connector::Rustls(Arc::new(config))))
}

/// 关掉证书与主机名校验的校验器。
///
/// 不挂根证书：校验器已经全部放行，加根也没有意义。TLS 1.2 与 1.3 都开，老服务端也能连。
#[derive(Debug)]
struct AcceptAny;

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
#[path = "ws_test.rs"]
mod tests;
