//! `ws.rs` 的测试：状态推进、双向帧、关闭事件。
//!
//! 服务端用 `tokio_tungstenite::accept_async`：这里要验的正是协议本身（握手、帧、关闭码），
//! 而不是我们自己的封装。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;

use super::*;

/// 回显服务器：把收到的每一帧原样送回。`port` 交给客户端。
async fn spawn_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let Ok(ws) = tokio_tungstenite::accept_async(sock).await else {
                    return;
                };
                let (mut write, mut read) = ws.split();
                while let Some(Ok(message)) = read.next().await {
                    let echo = match message {
                        Message::Text(text) => Message::Text(text),
                        Message::Binary(bytes) => Message::Binary(bytes),
                        Message::Close(frame) => {
                            let _ = write.send(Message::Close(frame)).await;
                            return;
                        }
                        _ => continue,
                    };
                    if write.send(echo).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

fn collector() -> (Arc<Mutex<Vec<WsEvent>>>, WsSink) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&events);
    let sink: WsSink = Box::new(move |event| {
        sink_events.lock().unwrap().push(event);
        true
    });
    (events, sink)
}

async fn wait_for<F>(events: &Arc<Mutex<Vec<WsEvent>>>, predicate: F) -> bool
where
    F: Fn(&WsEvent) -> bool,
{
    for _ in 0..100 {
        if events.lock().unwrap().iter().any(&predicate) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

fn text_frames(events: &[WsEvent], direction: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            WsEvent::Frame {
                direction: d,
                data: Some(text),
                ..
            } if d == direction => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_text_frame_round_trips_and_closing_reports_the_session() {
    let port = spawn_echo_server().await;
    let id = "ws-text".to_string();
    let (events, sink) = collector();
    connect(
        id.clone(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );

    assert!(
        wait_for(
            &events,
            |e| matches!(e, WsEvent::State { state } if state == "open")
        )
        .await
    );
    send(&id, "text", Some("hello"), None).expect("send");
    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Frame { direction, data: Some(text), .. } if direction == "in" && text == "hello"
        ))
        .await,
        "回显没有回来：{:?}",
        events.lock().unwrap()
    );

    let snapshot = events.lock().unwrap().clone();
    assert_eq!(text_frames(&snapshot, "out"), vec!["hello".to_string()]);
    assert_eq!(text_frames(&snapshot, "in"), vec!["hello".to_string()]);

    close(&id, Some(1000), "done").expect("close");
    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Closed { code: Some(1000), reason, .. } if reason == "done"
        ))
        .await,
        "关闭事件没有到：{:?}",
        events.lock().unwrap()
    );
}

#[tokio::test]
async fn a_binary_frame_is_reported_as_base64_with_the_raw_byte_count() {
    let port = spawn_echo_server().await;
    let id = "ws-binary".to_string();
    let (events, sink) = collector();
    connect(
        id.clone(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );

    assert!(
        wait_for(
            &events,
            |e| matches!(e, WsEvent::State { state } if state == "open")
        )
        .await
    );
    // "AQID" = 0x01 0x02 0x03
    send(&id, "binary", None, Some("AQID")).expect("send");

    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Frame { direction, kind, base64: Some(b64), bytes: 3, .. }
                if direction == "in" && kind == "binary" && b64 == "AQID"
        ))
        .await,
        "二进制回显没有回来：{:?}",
        events.lock().unwrap()
    );
    close(&id, None, "").expect("close");
}

#[tokio::test]
async fn sending_on_an_unknown_session_is_not_found() {
    let error = send("no-such-session", "text", Some("x"), None).unwrap_err();
    assert!(matches!(error, AppError::NotFound(_)), "got {error:?}");
}

#[tokio::test]
async fn an_invalid_url_is_reported_as_a_protocol_error() {
    let (events, sink) = collector();
    connect(
        "ws-bad-url".into(),
        "not a websocket url".into(),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );
    assert!(
        wait_for(
            &events,
            |e| matches!(e, WsEvent::Error { kind, .. } if kind == "protocol")
        )
        .await,
        "应当报协议错误：{:?}",
        events.lock().unwrap()
    );
}

// TLS 合成。不真起 TLS 服务端：`ws_connector` 是纯函数，错误路径与「用不用自建配置」
// 都能在构造阶段断出来。

#[test]
fn tls_settings_that_configure_nothing_use_the_builtin_roots() {
    let connector = ws_connector(&TlsSettings::default()).expect("不做任何配置不该报错");
    assert!(connector.is_none(), "空设置应当留给内置根证书");
}

#[test]
fn skip_verify_builds_its_own_connector() {
    let tls = TlsSettings {
        skip_verify: true,
        ..Default::default()
    };
    let connector = ws_connector(&tls).expect("跳过校验不需要读文件");
    assert!(matches!(connector, Some(Connector::Rustls(_))));
}

#[test]
fn a_missing_ca_file_is_an_error_that_names_the_path() {
    let path = std::env::temp_dir().join("simple-post-ws-missing-ca-do-not-create.pem");
    let path_text = path.display().to_string();
    let tls = TlsSettings {
        ca_certificate_path: Some(path_text.clone()),
        ..Default::default()
    };
    let err = match ws_connector(&tls) {
        Err(err) => err,
        Ok(_) => panic!("读不到 CA 应当报错"),
    };
    let message = err.to_string();
    assert!(message.contains(&path_text), "报文要点名路径：{message}");
    assert!(!path.exists(), "测试不该创建这个文件");
}

#[test]
fn a_client_certificate_without_its_key_is_rejected() {
    let path = std::env::temp_dir().join("simple-post-ws-cert-only.pem");
    let tls = TlsSettings {
        client_certificate_path: Some(path.display().to_string()),
        client_key_path: None,
        ..Default::default()
    };
    let err = match ws_connector(&tls) {
        Err(err) => err,
        Ok(_) => panic!("只有证书没有私钥应当报错"),
    };
    let message = err.to_string();
    assert!(message.contains("私钥"), "报文要说明缺私钥：{message}");
}
