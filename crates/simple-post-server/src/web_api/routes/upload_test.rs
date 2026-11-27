//! 上传路线：信封 + 文件字节的 `multipart/form-data` 请求。
//!
//! 跑真实 listener + 真实上游：服务端边收边转发的字节必须**真的**到上游，而不是只在回包里
//! 写着「成功」。分片顺序、文件名、声明长度与实收字节的对照都在这里钉住。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个临时端口。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-upload-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");

    let pool = crate::db::init(&dir).expect("db init");
    let state = WebState {
        pool,
        daemon: crate::daemon_supervisor::McpDaemonHandle::new(),
        data_dir: dir.clone(),
        password: None,
        base_path: None,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let router = build_router(state, spa_dir);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 回固定响应的假上游，并把收到的原始请求（头 + 体）记下来。
///
/// body 超过一次读取的量时会拼接到 `Content-Length` 为止：上传路线要断言的是整段字节。
async fn upstream() -> (String, Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut raw = Vec::new();
            let mut buf = vec![0u8; 16 * 1024];
            while let Ok(n) = sock.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if head_and_body_complete(&raw) {
                    break;
                }
            }
            sink.lock().unwrap().push(raw);
            let body = r#"{"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    (format!("http://127.0.0.1:{port}"), seen)
}

/// 头拿到 `Content-Length` 并且体收齐了。
fn head_and_body_complete(raw: &[u8]) -> bool {
    let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_ascii_lowercase();
    let Some(value) = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
    else {
        // 没有 `Content-Length`：读到关闭为止（这些用例都带长度）。
        return false;
    };
    let expected: usize = value.trim().parse().unwrap_or(0);
    raw.len() >= head_end + 4 + expected
}

/// 拆出请求头文本与请求体字节。头按文本断言，体按字节断言（上传的字节不保证是 UTF-8）。
fn split_request(raw: &[u8]) -> (String, Vec<u8>) {
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("request has a header block");
    (
        String::from_utf8_lossy(&raw[..head_end]).to_string(),
        raw[head_end + 4..].to_vec(),
    )
}

fn envelope(upstream: &str, body: serde_json::Value, mode: &str) -> String {
    serde_json::json!({
        "method": "POST",
        "url": format!("{upstream}/upload"),
        "headers": [],
        "body": body,
        "bodyMode": mode,
        "timeoutMs": 5000,
        "followRedirects": true,
        "maxResponseSizeKb": 1024,
    })
    .to_string()
}

#[tokio::test]
async fn files_arrive_in_part_order_with_their_names() {
    let (addr, _dir) = serve_test_router().await;
    let (upstream_url, seen) = upstream().await;

    let parts = serde_json::json!([
        { "name": "note", "value": "hello", "enabled": true },
        { "name": "one", "value": "", "enabled": true, "filename": "a.txt", "size": 5 },
        { "name": "two", "value": "", "enabled": true, "filename": "b.bin", "size": 4 },
    ]);
    let form = reqwest::multipart::Form::new()
        .part(
            "request",
            reqwest::multipart::Part::text(envelope(
                &upstream_url,
                serde_json::json!(parts.to_string()),
                "multipart",
            )),
        )
        .part(
            "a.txt",
            reqwest::multipart::Part::bytes(b"AAAAA".to_vec()).file_name("a.txt"),
        )
        .part(
            "b.bin",
            reqwest::multipart::Part::bytes(b"BBBB".to_vec()).file_name("b.bin"),
        );

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-upload"))
        .multipart(form)
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);
    let outcome: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(outcome["http"]["ok"], true, "outcome: {outcome}");

    let captured = seen
        .lock()
        .unwrap()
        .first()
        .cloned()
        .expect("upstream saw it");
    let (head, raw_body) = split_request(&captured);
    let body = String::from_utf8_lossy(&raw_body).to_string();
    let lower = head.to_ascii_lowercase();
    assert!(
        lower.contains("content-type: multipart/form-data; boundary="),
        "head: {head}"
    );
    // 声明长度就是实发长度：三段（文本 + 两个文件）都按分片头包进来。
    let declared: usize = lower
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .expect("content-length")
        .trim()
        .parse()
        .expect("numeric content-length");
    assert_eq!(declared, raw_body.len(), "body: {body}");

    let text_at = body.find("hello").expect("text part");
    let one_at = body.find("AAAAA").expect("first file");
    let two_at = body.find("BBBB").expect("second file");
    assert!(
        text_at < one_at && one_at < two_at,
        "part order must follow the parts array: {body}"
    );
    assert!(body.contains(r#"filename="a.txt""#), "body: {body}");
    assert!(body.contains(r#"filename="b.bin""#), "body: {body}");
}

#[tokio::test]
async fn a_declared_size_longer_than_the_bytes_aborts_before_the_body_ends() {
    let (addr, _dir) = serve_test_router().await;
    let (upstream_url, _seen) = upstream().await;

    let parts = serde_json::json!([
        { "name": "one", "value": "", "enabled": true, "filename": "a.txt", "size": 10 },
    ]);
    let form = reqwest::multipart::Form::new()
        .part(
            "request",
            reqwest::multipart::Part::text(envelope(
                &upstream_url,
                serde_json::json!(parts.to_string()),
                "multipart",
            )),
        )
        .part(
            "a.txt",
            reqwest::multipart::Part::bytes(b"AAAAA".to_vec()).file_name("a.txt"),
        );

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-upload"))
        .multipart(form)
        .send()
        .await
        .expect("request completes");
    // 执行器的失败装在 200 信封里（`http.ok = false`），不是 HTTP 状态码——与 `/send` 一致。
    assert_eq!(response.status(), 200);
    let text = response.text().await.expect("json body");
    assert!(
        text.contains("shorter than the declared size"),
        "body: {text}"
    );
}

#[tokio::test]
async fn a_zero_byte_binary_file_is_still_sent() {
    let (addr, _dir) = serve_test_router().await;
    let (upstream_url, seen) = upstream().await;

    // 零字节文件也是上传：信封带 `path: ""` 与 `size: 0`，上游要收到 `content-length: 0`
    // 而不是「没有请求体」。
    let description = serde_json::json!({
        "name": "empty.bin",
        "size": 0,
        "contentType": "application/octet-stream",
        "path": "",
    });
    let form = reqwest::multipart::Form::new()
        .part(
            "request",
            reqwest::multipart::Part::text(envelope(
                &upstream_url,
                serde_json::json!(description.to_string()),
                "binary",
            )),
        )
        .part(
            "empty.bin",
            reqwest::multipart::Part::bytes(Vec::new()).file_name("empty.bin"),
        );

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-upload"))
        .multipart(form)
        .send()
        .await
        .expect("request completes");
    let status = response.status();
    let text = response.text().await.expect("json body");
    assert_eq!(status, 200, "body: {text}");
    let outcome: serde_json::Value = serde_json::from_str(&text).expect("json body");
    assert_eq!(outcome["http"]["ok"], true, "outcome: {outcome}");

    let captured = seen
        .lock()
        .unwrap()
        .first()
        .cloned()
        .expect("upstream saw the request");
    let (head, body) = split_request(&captured);
    let head = head.to_ascii_lowercase();
    assert!(body.is_empty(), "zero-byte body, got {} bytes", body.len());
    assert!(
        head.contains("content-length: 0"),
        "content-length must be 0:\n{head}"
    );
}

#[tokio::test]
async fn a_binary_body_is_the_uploaded_bytes() {
    let (addr, _dir) = serve_test_router().await;
    let (upstream_url, seen) = upstream().await;

    let description = serde_json::json!({
        "name": "blob.bin",
        "size": 8,
        "contentType": "application/octet-stream",
        "path": "",
    });
    let form = reqwest::multipart::Form::new()
        .part(
            "request",
            reqwest::multipart::Part::text(envelope(
                &upstream_url,
                serde_json::json!(description.to_string()),
                "binary",
            )),
        )
        .part(
            "blob.bin",
            reqwest::multipart::Part::bytes(b"\x00\x01\x02DATA\xff".to_vec()),
        );

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-upload"))
        .multipart(form)
        .send()
        .await
        .expect("request completes");
    let status = response.status();
    let text = response.text().await.expect("json body");
    assert_eq!(status, 200, "body: {text}");
    let outcome: serde_json::Value = serde_json::from_str(&text).expect("json body");
    assert_eq!(outcome["http"]["ok"], true, "outcome: {outcome}");

    let captured = seen
        .lock()
        .unwrap()
        .first()
        .cloned()
        .expect("upstream saw it");
    let (head, body) = split_request(&captured);
    assert_eq!(body, b"\x00\x01\x02DATA\xff", "head: {head}");
}
