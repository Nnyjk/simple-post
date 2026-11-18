//! `/api/ws/sessions` 的 SSE 桥：响应头、事件序列、以及下行队列的背压取舍。
//!
//! 建连本身不在这里替身：core 的 `ws::connect` 是真实拨号，故测试指向一个**已经关掉的
//! 环回端口**——握手立刻以 `network` 失败，事件序列（`connecting` → `error`）完全确定，
//! 不依赖任何外部服务。

use std::time::Duration;

use tokio::sync::mpsc;

use simple_post_core::ws::WsEvent;

/// 只挂 `ws_connect` 一条路由：handler 不碰数据库。
async fn serve_route() -> std::net::SocketAddr {
    let app = axum::Router::new().route("/api/ws/sessions", axum::routing::post(super::ws_connect));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind router");
    let addr = listener.local_addr().expect("router addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// 拿一个当前没人监听的环回端口：绑上再立刻释放。
async fn closed_port() -> u16 {
    tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe")
        .local_addr()
        .expect("probe addr")
        .port()
}

fn state_event() -> WsEvent {
    WsEvent::State {
        state: "connecting".into(),
    }
}

/// `sessionId` 为空时 400：这条 id 是关闭会话的唯一钥匙。
#[tokio::test]
async fn an_empty_session_id_is_rejected() {
    let addr = serve_route().await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/ws/sessions"))
        .json(&serde_json::json!({ "sessionId": "   ", "url": "ws://127.0.0.1:1/" }))
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["code"], "validation", "body: {body}");
}

/// 建连失败也必须是一条**可用**的 SSE：头对、事件是 JSON、最后一条是 error。
#[tokio::test]
async fn the_sse_bridge_disables_proxy_buffering_and_streams_the_connecting_state() {
    let addr = serve_route().await;
    let port = closed_port().await;
    let session_id = format!("ws-test-bridge-{}", std::process::id());

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/ws/sessions"))
        .json(&serde_json::json!({
            "sessionId": session_id,
            "url": format!("ws://127.0.0.1:{port}/"),
        }))
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "content-type 应为 text/event-stream：{content_type}"
    );
    let buffering = response
        .headers()
        .get("x-accel-buffering")
        .and_then(|v| v.to_str().ok());
    assert_eq!(
        buffering,
        Some("no"),
        "缺 X-Accel-Buffering: no 时 nginx 会把整条流攒成一段再发"
    );

    let body = tokio::time::timeout(Duration::from_secs(10), response.text())
        .await
        .expect("建连失败后流应立刻收尾")
        .expect("读响应体");
    assert!(
        body.contains(r#""type":"state""#) && body.contains(r#""state":"connecting""#),
        "首帧必须是 connecting（登记先于建连）：{body}"
    );
    assert!(
        body.contains(r#""type":"error""#),
        "连不上的端口应回一条 error：{body}"
    );
    assert!(
        !body.contains("data: {}"),
        "序列化失败不该退化成空帧：{body}"
    );
}

/// 下行队列满 / 接收端消失时 sink 返回 `false`，让 core 的读循环停下——
/// 否则浏览器慢读会把整个下行堆在内存里，而下行空闲时的断连又没人收尾。
#[test]
fn a_stalled_downlink_stops_the_session() {
    let (tx, _rx) = mpsc::channel::<WsEvent>(1);
    tx.try_send(state_event()).expect("容量 1");
    let mut full = super::sink_into(tx, "ws-test-full".into());
    assert!(
        !full(state_event()),
        "队列满时必须让 core 停下，而不是丢一条继续读"
    );

    let (tx, rx) = mpsc::channel::<WsEvent>(1);
    drop(rx);
    let mut closed = super::sink_into(tx, "ws-test-closed".into());
    assert!(!closed(state_event()), "接收端消失时必须让 core 停下");
}
