//! `/api/http/stream` 的线形状与背压取舍。
//!
//! 两件在单元层面看不见、只有走真实 socket 才暴露的事：
//!
//!   * `X-Accel-Buffering: no` 必须出现在响应上——nginx 默认 `proxy_buffering on` 会把
//!     SSE 攒成整段再发，这条头是唯一的逐响应关闭开关。
//!   * 下行队列有界之后，「浏览器慢读」必须收掉会话而不是无限堆积。`sink_into` 的
//!     Full / Closed 两条分支都用真实注册表里的会话验一遍（`cancel` 返回 `false` 即
//!     说明 sink 已经把它摘掉了）。

use std::time::Duration;

use tokio::sync::mpsc;

use simple_post_core::http_executor::HttpRequestInput;
use simple_post_core::stream::{self, StreamEvent};

/// 只回一次固定响应的上游：读完请求就断开。
async fn upstream(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let port = listener.local_addr().expect("upstream addr").port();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8 * 1024];
                let _ = sock.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.flush().await;
            });
        }
    });
    format!("http://127.0.0.1:{port}/events")
}

/// 接受连接但永不回包的上游：让流任务一直停在等响应头上，会话因此留在注册表里。
async fn black_hole() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind black hole");
    let port = listener.local_addr().expect("black hole addr").port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((sock, _)) = listener.accept().await {
            held.push(sock);
        }
    });
    format!("http://127.0.0.1:{port}/hang")
}

/// 只挂 `http_stream` 一条路由：这两个 handler 不碰数据库，故不需要整个 `WebState`。
async fn serve_route() -> std::net::SocketAddr {
    let app =
        axum::Router::new().route("/api/http/stream", axum::routing::post(super::http_stream));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind router");
    let addr = listener.local_addr().expect("router addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

fn input(url: &str) -> HttpRequestInput {
    HttpRequestInput {
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
        body_mode: simple_post_core::domain::BodyMode::None,
        timeout_ms: 60_000,
        follow_redirects: false,
        max_response_size_kb: 1024,
        tls: simple_post_core::tls::resolve(
            &simple_post_core::domain::TlsSettings::default(),
            None,
        ),
    }
}

fn end_event() -> StreamEvent {
    StreamEvent::End {
        duration_ms: 0,
        size_bytes: 0,
        truncated: false,
        reason: "eof".into(),
    }
}

/// 响应头、事件序列与收尾：正常的流必须以 `end{eof}` 结束，且没有任何 `{}` 占位帧。
#[tokio::test]
async fn the_sse_response_disables_proxy_buffering_and_closes_with_an_end_event() {
    let url = upstream("hello").await;
    let addr = serve_route().await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/stream"))
        .json(&serde_json::json!({
            "streamId": format!("stream-test-ok-{}", std::process::id()),
            "method": "GET",
            "url": url,
            "timeoutMs": 5_000,
            "followRedirects": true,
            "maxResponseSizeKb": 1024,
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

    let body = tokio::time::timeout(Duration::from_secs(5), response.text())
        .await
        .expect("流应在 end 之后收尾")
        .expect("读响应体");
    assert!(body.contains(r#""type":"meta""#), "缺 meta 帧：{body}");
    assert!(body.contains(r#""type":"chunk""#), "缺 chunk 帧：{body}");
    assert!(
        body.contains(r#""type":"end""#) && body.contains(r#""reason":"eof""#),
        "正常收尾必须送 end{{eof}}：{body}"
    );
    assert!(
        !body.contains("data: {}"),
        "序列化失败不该退化成空帧：{body}"
    );
}

/// 下行队列满或接收端消失时，sink 必须把会话取消掉——不是丢弃单条事件继续读。
#[tokio::test]
async fn a_stalled_downlink_cancels_the_stream() {
    let url = black_hole().await;
    let pid = std::process::id();

    for stalled in ["full", "closed"] {
        let id = format!("stream-test-{stalled}-{pid}");
        // `spawn` 先写注册表再起任务，故这一行之后会话一定在。
        stream::spawn(id.clone(), input(&url), Box::new(|_| {}));

        // capacity 1：`full` 先塞满；`closed` 把接收端丢掉。
        let (tx, rx) = mpsc::channel::<StreamEvent>(1);
        if stalled == "full" {
            tx.try_send(end_event()).expect("容量 1");
        } else {
            drop(rx);
        }
        let mut sink = super::sink_into(tx, id.clone());
        sink(end_event());

        assert!(
            !stream::cancel(&id),
            "{stalled}: sink 应在投递失败时取消会话，而不是继续堆积"
        );
    }
}
