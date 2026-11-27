//! `stream.rs` 的测试：事件顺序、空闲超时、上限裁剪与取消。
//!
//! 服务器是裸 TCP：这里要控制的正是「响应头怎么来、块怎么分开发」，测试框架会把这两件事藏
//! 起来。请求头读完之后才写响应头，否则客户端会先收到响应再发请求。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;

/// 起一个 SSE 服务器：`chunks` 逐块发出，块之间等 `gap`，然后关闭连接。
async fn spawn_sse_server(chunks: Vec<Vec<u8>>, gap: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let chunks = chunks.clone();
            tokio::spawn(async move {
                read_head(&mut sock).await;
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\n\
                          Content-Type: text/event-stream\r\n\
                          Transfer-Encoding: chunked\r\n\r\n",
                    )
                    .await;
                for chunk in chunks {
                    let head = format!("{:x}\r\n", chunk.len());
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    if sock.write_all(chunk.as_slice()).await.is_err() {
                        return;
                    }
                    if sock.write_all(b"\r\n").await.is_err() {
                        return;
                    }
                    tokio::time::sleep(gap).await;
                }
                let _ = sock.write_all(b"0\r\n\r\n").await;
            });
        }
    });
    port
}

async fn read_head(sock: &mut TcpStream) {
    let mut seen = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let Ok(n) = sock.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            return;
        }
        seen.extend_from_slice(&buf[..n]);
        if seen.windows(4).any(|w| w == b"\r\n\r\n") {
            return;
        }
    }
}

fn input(port: u16, timeout_ms: u64, max_kb: u64) -> HttpRequestInput {
    HttpRequestInput {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/events"),
        headers: vec![("Accept".into(), "text/event-stream".into())],
        body: None,
        body_mode: crate::domain::BodyMode::None,
        timeout_ms,
        follow_redirects: true,
        max_response_size_kb: max_kb,
        tls: crate::domain::TlsSettings::default(),
    }
}

/// 收集事件的出口。`run` 在本任务里 await，故共享一个 `Arc<Mutex<…>>` 就够了。
fn collector() -> (Arc<Mutex<Vec<StreamEvent>>>, StreamSink) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&events);
    let sink: StreamSink = Box::new(move |event| {
        sink_events.lock().unwrap().push(event);
    });
    (events, sink)
}

#[tokio::test]
async fn meta_chunks_and_end_arrive_in_order() {
    let port = spawn_sse_server(
        vec![b"data: one\n\n".to_vec(), b"data: two\n\n".to_vec()],
        Duration::from_millis(10),
    )
    .await;
    let (events, sink) = collector();
    run(input(port, 5_000, 0), sink).await;

    let events = events.lock().unwrap();
    assert!(matches!(
        &events[0],
        StreamEvent::Meta { status: 200, content_type, .. } if content_type.starts_with("text/event-stream")
    ));
    assert_eq!(
        events[1],
        StreamEvent::Chunk {
            seq: 0,
            text: "data: one\n\n".into(),
            bytes: 11,
        }
    );
    assert_eq!(
        events[2],
        StreamEvent::Chunk {
            seq: 1,
            text: "data: two\n\n".into(),
            bytes: 11,
        }
    );
    match events.last().unwrap() {
        StreamEvent::End {
            size_bytes,
            truncated,
            reason,
            ..
        } => {
            assert_eq!(*size_bytes, 22);
            assert!(!truncated);
            assert_eq!(reason, "eof");
        }
        other => panic!("最后一个事件应当是 end，got {other:?}"),
    }
}

#[tokio::test]
async fn an_idle_gap_longer_than_the_timeout_ends_the_stream_with_timeout() {
    let port = spawn_sse_server(vec![b"data: one\n\n".to_vec()], Duration::from_millis(500)).await;
    let (events, sink) = collector();
    run(input(port, 100, 0), sink).await;

    let events = events.lock().unwrap();
    match events.last().unwrap() {
        StreamEvent::End { reason, .. } => assert_eq!(reason, "timeout"),
        other => panic!("最后一个事件应当是 end(timeout)，got {other:?}"),
    }
}

#[tokio::test]
async fn a_response_over_the_cap_ends_truncated_at_the_cap() {
    // 2 KB 的块，上限 1 KB。
    let port = spawn_sse_server(vec![vec![b'x'; 2048]], Duration::from_millis(10)).await;
    let (events, sink) = collector();
    run(input(port, 5_000, 1), sink).await;

    let events = events.lock().unwrap();
    match events.last().unwrap() {
        StreamEvent::End {
            size_bytes,
            truncated,
            reason,
            ..
        } => {
            assert_eq!(*size_bytes, 1024);
            assert!(*truncated);
            assert_eq!(reason, "eof");
        }
        other => panic!("最后一个事件应当是 end(truncated)，got {other:?}"),
    }
}

#[tokio::test]
async fn cancel_reports_whether_the_session_was_registered() {
    let port = spawn_sse_server(vec![b"data: one\n\n".to_vec()], Duration::from_secs(5)).await;
    let (events, sink) = collector();
    spawn("sess-1".into(), input(port, 5_000, 0), sink);
    // 等任务登记进注册表并在服务端建好连接。
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(cancel("sess-1"));
    assert!(!cancel("sess-1"));

    // 收尾事件要真的到达：面板靠它把状态改成「已停止」并写 response_history。
    tokio::time::sleep(Duration::from_millis(100)).await;
    let events = events.lock().unwrap();
    assert!(
        events.iter().any(|e| matches!(e, StreamEvent::Meta { .. })),
        "取消之前至少该收到 meta，got {events:?}"
    );
    match events.last() {
        Some(StreamEvent::End {
            reason, truncated, ..
        }) => {
            assert_eq!(reason, "stopped");
            assert!(!truncated);
        }
        other => panic!("取消后应以 end{{stopped}} 收尾，got {other:?}"),
    }
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, StreamEvent::Error { .. })),
        "取消不是错误，got {events:?}"
    );
}

#[tokio::test]
async fn a_character_split_across_chunks_is_decoded_once() {
    // “中” 的 UTF-8 是 E4 B8 AD；把前两个字节与第三个字节分两条块送，模拟网络切分。
    let port = spawn_sse_server(
        vec![vec![0xE4, 0xB8], vec![0xAD, b'x']],
        Duration::from_millis(10),
    )
    .await;
    let (events, sink) = collector();
    run(input(port, 5_000, 0), sink).await;

    let events = events.lock().unwrap();
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Chunk { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "中x");
    assert!(!text.contains('\u{FFFD}'), "got {text:?}");
}

#[tokio::test]
async fn a_cap_cutting_a_multibyte_character_drops_the_half_without_a_replacement_char() {
    // 上限 1 KB，块是 1023 个 x 加一个「中」（E4 B8 AD）：切点正好落在「中」的第一个字节
    // 之后。半截字节属于被截掉的那一半，直接丢——吐成 U+FFFD 等于凭空造了一个字符。
    let mut body = vec![b'x'; 1023];
    body.extend_from_slice("中".as_bytes());
    let port = spawn_sse_server(vec![body], Duration::from_millis(10)).await;
    let (events, sink) = collector();
    run(input(port, 5_000, 1), sink).await;

    let events = events.lock().unwrap();
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Chunk { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "x".repeat(1023));
    assert!(
        !text.contains('\u{FFFD}'),
        "截断的残留应当直接丢，got {text:?}"
    );
    match events.last().unwrap() {
        StreamEvent::End {
            size_bytes,
            truncated,
            reason,
            ..
        } => {
            assert_eq!(*size_bytes, 1024);
            assert!(*truncated);
            assert_eq!(reason, "eof");
        }
        other => panic!("最后一个事件应当是 end(truncated)，got {other:?}"),
    }
}

#[tokio::test]
async fn an_invalid_url_is_reported_as_a_network_error_without_dialing() {
    let mut req = input(1, 1_000, 0);
    req.url = "ftp://example.test/file".into();
    let (events, sink) = collector();
    run(req, sink).await;

    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Error { kind, message } => {
            assert_eq!(kind, "network");
            assert!(message.contains("scheme"), "got {message}");
        }
        other => panic!("应当是 error，got {other:?}"),
    }
}

/// 服务器接受了连接却一直不回响应头：超时要以 `end{timeout}` 收尾，不是 `error`。
#[tokio::test]
async fn a_server_that_never_sends_headers_ends_with_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            // 只读请求头，然后保持连接静默——比 `timeoutMs` 活得久。
            tokio::time::sleep(Duration::from_secs(2)).await;
            drop(sock);
        }
    });

    let (events, sink) = collector();
    run(input(port, 100, 0), sink).await;

    let events = events.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, StreamEvent::Error { .. })),
        "超时不是错误：{events:?}"
    );
    assert!(
        matches!(
            events.last(),
            Some(StreamEvent::End { reason, .. }) if reason == "timeout"
        ),
        "最后一条必须是 end(timeout)：{events:?}"
    );
}
