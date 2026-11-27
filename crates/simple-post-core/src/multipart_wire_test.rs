//! 真实抓包：起一个 TCP 监听器，用产品自己的 `http_executor` 发一次 multipart / binary，
//! 把服务端收到的**原始字节**读回来解析。
//!
//! 与同目录 `multipart_test.rs` 的区别：那里断言装配出来的字节长这样，这里断言 socket 上
//! 真的过去的是这个。`Content-Length`、`Content-Type` 的 boundary 与 body 里的 boundary
//! 是否同一个、文件字节有没有被少发——这些只有真实发送看得见。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use crate::domain::BodyMode;
use crate::http_executor::{execute_with_policy, HttpRequestInput, OutboundPolicy};

/// 服务端收到的一份原始请求，头与 body 分开。
struct Captured {
    head: String,
    body: Vec<u8>,
}

impl Captured {
    /// 按 header 名取一个值，不区分大小写。
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

/// 从 `Content-Type` 里取出 boundary。这是 RFC 7578 的参数，不是我们自己塞进去的标记，
/// 所以从这里取的值就是服务端解析时会用的那个。
fn declared_boundary(captured: &Captured) -> String {
    captured
        .header("Content-Type")
        .expect("multipart request must carry a Content-Type")
        .strip_prefix("multipart/form-data; boundary=")
        .expect("Content-Type must be multipart/form-data with a boundary")
        .to_string()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| v.trim().parse().ok())?
    })
}

/// 读满一个 HTTP/1.1 请求：先读到头结束，再按 `Content-Length` 读够 body。
///
/// `Content-Length` 缺失就当 chunked 处理，逐块读到终止块——不做这个区分的话，一个忘带
/// `Content-Length` 的 body 会被安静地少读几个字节，而断言仍然可能通过。
fn read_request(stream: &mut std::net::TcpStream) -> Captured {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");

    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut split_at = None;

    loop {
        let n = stream.read(&mut chunk).expect("read request");
        assert_ne!(n, 0, "connection closed before the request was complete");
        buf.extend_from_slice(&chunk[..n]);

        if split_at.is_none() {
            split_at = find(&buf, b"\r\n\r\n").map(|i| i + 4);
        }
        let Some(at) = split_at else { continue };

        let head = String::from_utf8_lossy(&buf[..at]).to_string();
        if let Some(len) = content_length(&head) {
            if buf.len() >= at + len {
                return Captured {
                    body: buf[at..at + len].to_vec(),
                    head,
                };
            }
        } else if let Some(end) = find(&buf[at..], b"0\r\n\r\n") {
            return Captured {
                body: buf[at..at + end].to_vec(),
                head,
            };
        }
    }
}

/// 跑一次真实的发送，返回服务端收到的字节。
fn capture(method: &str, headers: Vec<(String, String)>, body: &str, mode: BodyMode) -> Captured {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local_addr");

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let captured = read_request(&mut stream);
        // 必须回一个响应：`execute` 要读到响应头才算完，否则它会一直等到自己的超时，
        // 而那时这条测试报出来的是「请求失败」，与请求体无关。
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .expect("respond");
        stream.flush().expect("flush");
        captured
    });

    let input = HttpRequestInput {
        method: method.to_string(),
        url: format!("http://{addr}/upload"),
        headers,
        body: Some(body.to_string()),
        body_mode: mode,
        timeout_ms: 10_000,
        follow_redirects: false,
        max_response_size_kb: 1024,
        tls: Default::default(),
    };

    // 显式给策略，别让 CI 环境变量决定这条测试的行为。
    let policy = OutboundPolicy {
        block_private_net: false,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let result = execute_with_policy(&input, policy).await;
            assert!(
                result.ok,
                "the request must complete: {:?}",
                result.error.as_ref().map(|e| &e.message)
            );
        });

    server.join().expect("server thread panicked")
}

/// 把收到的 body 按它自己声明的 boundary 切开，还原成分片。这是独立于实现的解析，
/// 所以「我们以为的分片」与「服务端会读到的分片」不一致时，它会报出来。
fn split_parts(captured: &Captured) -> Vec<(Vec<(String, String)>, String)> {
    let boundary = declared_boundary(captured);
    let delimiter = format!("--{boundary}");
    let body = captured.body_text();

    let body = body
        .strip_prefix(&delimiter)
        .unwrap_or_else(|| panic!("body must open with {delimiter}, got {body:?}"));
    let body = body.strip_suffix("\r\n").unwrap_or(body);

    body.split(&format!("\r\n{delimiter}"))
        .filter(|chunk| !chunk.is_empty() && !chunk.starts_with("--"))
        .map(|chunk| {
            let (raw_headers, value) = chunk
                .split_once("\r\n\r\n")
                .unwrap_or_else(|| panic!("part has no header/body separator: {chunk:?}"));
            let headers = raw_headers
                .lines()
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
            (headers, value.to_string())
        })
        .collect()
}

fn parted(name: &str, value: &str) -> String {
    format!(r#"{{"name":"{name}","value":"{value}"}}"#)
}

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn with(bytes: &[u8]) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("simple-post-wire-{}-{seq}.bin", std::process::id()));
        std::fs::write(&path, bytes).expect("write the temp file");
        Self { path }
    }

    fn path_string(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn a_multipart_request_that_reaches_a_socket_is_parsable() {
    let file = TempFile::with(b"\x89PNG\r\n\x1a\nbinary");
    let body = format!(
        "[{},{{\"name\":\"file\",\"value\":\"\",\"filename\":\"a.png\",\"path\":{}}},{}]",
        parted("title", "报告"),
        serde_json::to_string(&file.path_string()).expect("json"),
        parted("note", ""),
    );

    let captured = capture(
        "POST",
        vec![("Accept".into(), "application/json".into())],
        &body,
        BodyMode::Multipart,
    );

    assert!(
        captured.head.starts_with("POST /upload HTTP/1.1"),
        "unexpected request line: {:?}",
        captured.head.lines().next()
    );

    // 声明的长度必须与 body 的实际字节数一致——少了就是服务端在等一个永远不来的字节。
    let declared = content_length(&captured.head).expect("Content-Length");
    assert_eq!(
        declared,
        captured.body.len(),
        "Content-Length must match the body actually written to the socket"
    );

    let parts = split_parts(&captured);
    assert_eq!(parts.len(), 3, "all three parts must survive: {parts:?}");

    let names: Vec<&str> = parts
        .iter()
        .map(|(headers, _)| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-disposition"))
                .and_then(|(_, v)| v.split("name=\"").nth(1))
                .and_then(|s| s.split('"').next())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        names,
        vec!["title", "file", "note"],
        "part names and order must be kept"
    );

    assert_eq!(
        parts[0].1, "报告",
        "a non-ASCII value must survive byte-for-byte"
    );

    let file_disposition = parts[1]
        .0
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-disposition"))
        .map(|(_, v)| v.clone())
        .expect("the file part must carry a Content-Disposition");
    assert!(
        file_disposition.contains(r#"filename="a.png""#),
        "the file part lost its filename: {file_disposition}"
    );
    let file_type = parts[1]
        .0
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());
    assert_eq!(
        file_type.as_deref(),
        Some("application/octet-stream"),
        "a file part with no explicit type must fall back to the binary stream type"
    );
    // 文件字节按**原始字节**比对：分片正文经过 lossy 解码后不可信，非 UTF-8 的字节
    // 会被替换成 U+FFFD，那样断言反而看不见真正的差异。
    assert!(
        captured
            .body
            .windows(b"\x89PNG\r\n\x1a\nbinary".len())
            .any(|w| w == b"\x89PNG\r\n\x1a\nbinary"),
        "the file's bytes must reach the socket unchanged"
    );

    assert!(
        !parts[0]
            .0
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type")),
        "a plain field must not carry its own Content-Type"
    );
    assert_eq!(parts[2].1, "");

    // 收尾分隔符：服务端据此知道没有更多分片了。
    assert!(
        captured
            .body_text()
            .ends_with(&format!("--{}--\r\n", declared_boundary(&captured))),
        "the closing delimiter is missing"
    );
}

#[test]
fn a_user_set_content_type_is_replaced_by_the_one_with_the_boundary() {
    let captured = capture(
        "POST",
        vec![("content-type".to_string(), "application/json".to_string())],
        &format!("[{}]", parted("a", "1")),
        BodyMode::Multipart,
    );

    let ct_headers: Vec<&str> = captured
        .head
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("content-type"))
        })
        .collect();
    assert_eq!(
        ct_headers.len(),
        1,
        "the Content-Type must be overwritten in place, not appended: {:#?}",
        captured.head
    );
    assert!(
        ct_headers[0]
            .to_ascii_lowercase()
            .contains("multipart/form-data; boundary="),
        "the value must be the multipart one: {ct_headers:?}"
    );

    let boundary = declared_boundary(&captured);
    assert!(
        captured.body_text().contains(&format!("--{boundary}\r\n")),
        "the declared boundary must be the one used in the body"
    );
}

#[test]
fn a_binary_body_sends_the_file_with_an_exact_length() {
    let file = TempFile::with(&vec![0xab; 100 * 1024 + 7]);
    let body = serde_json::json!({
        "name": "blob.bin",
        "size": 100 * 1024 + 7,
        "contentType": "application/octet-stream",
        "path": file.path_string(),
    })
    .to_string();

    let captured = capture(
        "POST",
        vec![(
            "Content-Type".to_string(),
            "application/octet-stream".to_string(),
        )],
        &body,
        BodyMode::Binary,
    );

    assert_eq!(
        content_length(&captured.head),
        Some(100 * 1024 + 7),
        "the declared length must be the file's size, not the JSON description's"
    );
    assert_eq!(captured.body.len(), 100 * 1024 + 7);
    assert!(captured.body.iter().all(|b| *b == 0xab));
}

#[test]
fn a_malformed_parts_payload_still_reaches_the_socket_as_valid_multipart() {
    // 零分片仍是格式合法的 body（只有收尾分隔符）。宁可让服务端报「没有分片」，
    // 也不要发一个内容被静默改写的 body。
    let captured = capture("POST", Vec::new(), "{ not json", BodyMode::Multipart);
    assert!(
        split_parts(&captured).is_empty(),
        "no part should be invented"
    );
    assert!(
        captured
            .body_text()
            .ends_with(&format!("--{}--\r\n", declared_boundary(&captured))),
        "still a well-formed multipart, just empty"
    );
}
