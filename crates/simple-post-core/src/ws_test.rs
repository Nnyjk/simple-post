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

// 会话生命周期：同 id 重连、出站策略、读侧出错、文本帧的载荷。

/// 握手完成后直接断开、不发关闭帧的服务端：客户端读到的是协议错误（缺关闭握手），不是 eof。
async fn spawn_drop_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let Ok((sock, _)) = listener.accept().await else {
            return;
        };
        if let Ok(ws) = tokio_tungstenite::accept_async(sock).await {
            // 丢掉 socket：FIN 没带关闭帧。
            drop(ws);
        }
    });
    port
}

async fn opened(events: &Arc<Mutex<Vec<WsEvent>>>) -> bool {
    wait_for(
        events,
        |e| matches!(e, WsEvent::State { state } if state == "open"),
    )
    .await
}

#[tokio::test]
async fn connecting_the_same_id_again_closes_the_old_session_and_spares_the_new_one() {
    let port = spawn_echo_server().await;
    let id = "ws-replace".to_string();

    let (old_events, old_sink) = collector();
    connect(
        id.clone(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        old_sink,
    );
    assert!(opened(&old_events).await, "第一条会话没连上");

    let (new_events, new_sink) = collector();
    connect(
        id.clone(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        new_sink,
    );
    assert!(opened(&new_events).await, "第二条会话没连上");

    // 旧任务只握着同一个 channel 的另一份 sender，故它自己等不到 `recv()==None`：必须有人
    // 显式投一条 `Close`，否则这条连接永远不收尾。
    assert!(
        wait_for(&old_events, |e| matches!(
            e,
            WsEvent::Closed { code: Some(1000), reason, .. } if reason == "会话已被同一 id 的新连接取代"
        ))
        .await,
        "旧会话没有收尾：{:?}",
        old_events.lock().unwrap()
    );

    // 旧任务收尾（`finish`）不能把刚登记的新会话摘掉：等它真的跑完，再发一帧。
    tokio::time::sleep(Duration::from_millis(150)).await;
    send(&id, "text", Some("still here"), None).expect("新会话被旧会话的收尾误删了");
    assert!(
        wait_for(&new_events, |e| matches!(
            e,
            WsEvent::Frame { direction, data: Some(text), .. }
                if direction == "in" && text == "still here"
        ))
        .await,
        "新会话不再收发：{:?}",
        new_events.lock().unwrap()
    );
    close(&id, Some(1000), "done").expect("close");
}

#[test]
fn the_websocket_schemes_are_judged_by_the_http_outbound_policy() {
    // 判定是纯函数，故不翻环境变量：`SIMPLE_POST_BLOCK_PRIVATE_NET` 是进程级的一个槽位，
    // 改它的窗口会把同一进程里别的 loopback 用例一起拒掉。
    let blocked = OutboundPolicy {
        block_private_net: true,
    };

    match check_ws_url("ws://127.0.0.1:9/socket", blocked) {
        UrlVerdict::Refused(reason) => {
            assert!(
                reason.contains("SIMPLE_POST_BLOCK_PRIVATE_NET"),
                "理由要说明是哪条策略：{reason}"
            );
            // 理由里是调用方填的那条 URL，不是内部换过的 http 探针。
            assert!(reason.contains("ws://127.0.0.1:9/socket"), "got {reason}");
            assert!(!reason.contains("http://"), "got {reason}");
        }
        UrlVerdict::Allowed => panic!("受限策略下 loopback 应当被拒"),
    }
    assert!(matches!(
        check_ws_url("wss://127.0.0.1:9/socket", blocked),
        UrlVerdict::Refused(_)
    ));
    assert!(matches!(
        check_ws_url("ws://10.0.0.7/socket", blocked),
        UrlVerdict::Refused(_)
    ));
    // 公网名字照旧放行；桌面默认（只有 scheme 白名单）也不受影响。
    assert_eq!(
        check_ws_url("wss://example.test/socket", blocked),
        UrlVerdict::Allowed
    );
    assert_eq!(
        check_ws_url("ws://127.0.0.1:9/socket", OutboundPolicy::default()),
        UrlVerdict::Allowed
    );
}

#[tokio::test]
async fn a_refused_url_is_a_protocol_error_and_never_opens() {
    // 端口越界是被 URL 解析拒的，与策略开关无关，故这条用例在任何环境变量下都成立；策略
    // 的私有网段判定本身由上一条用例直接验。要断言的是顺序：拒绝在拨号之前、`connecting`
    // 之后，只报 `error`，不补 `closed`。
    let (events, sink) = collector();
    connect(
        "ws-refused".into(),
        "ws://127.0.0.1:70000/socket".into(),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );
    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Error { kind, .. } if kind == "protocol"
        ))
        .await,
        "应当报协议错误：{:?}",
        events.lock().unwrap()
    );
    let snapshot = events.lock().unwrap().clone();
    assert!(
        !snapshot
            .iter()
            .any(|e| matches!(e, WsEvent::State { state } if state == "open")),
        "被拒的 URL 不该报 open：{snapshot:?}"
    );
    assert!(
        !snapshot.iter().any(|e| matches!(e, WsEvent::Closed { .. })),
        "被拒的 URL 不走 closed：{snapshot:?}"
    );
}

#[tokio::test]
async fn a_read_side_break_is_an_error_without_a_closed_event() {
    let port = spawn_drop_server().await;
    let (events, sink) = collector();
    connect(
        "ws-read-error".into(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );
    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Error { kind, .. } if kind == "network"
        ))
        .await,
        "读侧断开应当报网络错误：{:?}",
        events.lock().unwrap()
    );

    // 再补一条 `Closed` 会把面板的 phase 覆盖成「未连接」，与建连失败显示「连接失败」不一致。
    tokio::time::sleep(Duration::from_millis(150)).await;
    let snapshot = events.lock().unwrap().clone();
    assert!(
        !snapshot.iter().any(|e| matches!(e, WsEvent::Closed { .. })),
        "错误之后不该再补 closed：{snapshot:?}"
    );
    assert!(
        matches!(
            snapshot.last(),
            Some(WsEvent::Error { kind, .. }) if kind == "network"
        ),
        "最后一条应当是那条错误：{snapshot:?}"
    );
}

#[tokio::test]
async fn text_with_base64_is_rejected() {
    let error = send("no-such-session", "text", Some("hi"), Some("AQID")).unwrap_err();
    match error {
        AppError::Validation(message) => {
            assert!(message.contains("base64"), "报文要点名 base64：{message}");
            assert!(
                message.contains("data"),
                "报文要说清文本帧只给 data：{message}"
            );
        }
        other => panic!("带 base64 的文本帧应当是 Validation，got {other:?}"),
    }
}

#[tokio::test]
async fn a_plain_text_frame_still_goes_out() {
    let port = spawn_echo_server().await;
    let id = "ws-plain-text".to_string();
    let (events, sink) = collector();
    connect(
        id.clone(),
        format!("ws://127.0.0.1:{port}/socket"),
        Vec::new(),
        Vec::new(),
        None,
        sink,
    );
    assert!(opened(&events).await, "没连上");

    send(&id, "text", Some("plain"), None).expect("send");
    assert!(
        wait_for(&events, |e| matches!(
            e,
            WsEvent::Frame { direction, kind, data: Some(text), base64: None, .. }
                if direction == "out" && kind == "text" && text == "plain"
        ))
        .await,
        "文本帧没有发出去：{:?}",
        events.lock().unwrap()
    );
    close(&id, Some(1000), "done").expect("close");
}
