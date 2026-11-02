//! 真实抓包：起一个 TCP 监听器，用产品自己的 `http_executor` 发一次 multipart，
//! 把服务端收到的**原始字节**读回来解析。
//!
//! 与同目录其它用例的区别：那些断言「我们拼出的字符串长这样」，这条断言「socket 上
//! 真的过去的是这个」。`apply` 与 `http_executor` 之间若有编码、换行、头顺序或
//! `Content-Length` 的问题，只有这条看得见——而那些恰好都是「请求发得出去、服务端
//! 解析不出分片、用户看不出任何异常」那一类。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use simple_post_core::http_executor::{execute_with_policy, HttpRequestInput, OutboundPolicy};

use super::*;

/// 服务端收到的一份原始请求，头与 body 分开。
struct Captured {
    head: String,
    body: String,
}

impl Captured {
    /// 按 header 名取一个值，不区分大小写。
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

/// 从 `Content-Type` 里取出 boundary。这是 RFC 7578 的参数，不是我们自己塞进去的
/// 标记，所以从这里取的值就是服务端解析时会用的那个。
fn declared_boundary(captured: &Captured) -> &str {
    captured
        .header("Content-Type")
        .expect("multipart request must carry a Content-Type")
        .strip_prefix("multipart/form-data; boundary=")
        .expect("Content-Type must be multipart/form-data with a boundary")
}

/// 读满一个 HTTP/1.1 请求：先读到头结束，再按 `Content-Length` 读够 body。
///
/// `Content-Length` 缺失就当 chunked 处理，逐块读到终止块——不做这个区分的话，一个
/// 忘带 `Content-Length` 的 body 会被安静地少读几个字节，而断言仍然可能通过。
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
                    body: String::from_utf8_lossy(&buf[at..at + len]).to_string(),
                    head,
                };
            }
        } else if let Some(end) = find(&buf, b"0\r\n\r\n") {
            // chunked：到终止块为止。
            return Captured {
                body: String::from_utf8_lossy(&buf[at..end]).to_string(),
                head,
            };
        }
    }
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| v.trim().parse().ok())?
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 跑一次真实的发送，返回服务端收到的字节。`parts_json` 就是 `RequestBody.content`。
fn capture(parts_json: &str) -> Captured {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("local_addr");

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let captured = read_request(&mut stream);
        // 必须回一个响应：`execute` 要读到响应头才算完，否则它会一直等到自己的超时，
        // 而那时这条测试报出来的是「请求失败」，与 multipart 无关。
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .expect("respond");
        stream.flush().expect("flush");
        captured
    });

    // 与 `mcp::http_ops` 装请求的顺序一致：先让 `apply` 改写 headers 并给出 body，
    // 再把两者一起交给 `http_executor`。
    let mut headers: Vec<(String, String)> = vec![("Accept".into(), "application/json".into())];
    let assembled = apply(&mut headers, parts_json);
    assert!(
        headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type")),
        "`apply` must have put a Content-Type on the request"
    );

    let input = HttpRequestInput {
        method: "POST".into(),
        url: format!("http://{addr}/upload"),
        headers,
        // 文本形态与二进制形态互斥：把 `AssembledBody` 的两个字段原样交给传输层，
        // 这里不做任何判断——判断在装配那一步已经做完了。
        body: assembled.text,
        body_base64: assembled.base64,
        timeout_ms: 10_000,
        follow_redirects: false,
        max_response_size_kb: 1024,
        // 这条用例验的是 multipart 的字节形状，HTTPS 设置用默认值。
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
fn split_parts(captured: &Captured) -> Vec<(Vec<(&str, String)>, String)> {
    let boundary = declared_boundary(captured);
    let delimiter = format!("--{boundary}");

    let body = captured
        .body
        .strip_prefix(&delimiter)
        .unwrap_or_else(|| panic!("body must open with {delimiter}, got {:?}", captured.body));
    let body = body.strip_suffix("\r\n").unwrap_or(body);

    // 切出来的第一段是开头分隔符留下的空串，最后一段是收尾分隔符剩下的 `--`。
    // 真正的分片段都以 `\r\n` 开头，所以按这两个特征剔掉首尾。
    body.split(&format!("\r\n{delimiter}"))
        .filter(|chunk| !chunk.is_empty() && !chunk.starts_with("--"))
        .map(|chunk| {
            let (raw_headers, value) = chunk
                .split_once("\r\n\r\n")
                .unwrap_or_else(|| panic!("part has no header/body separator: {chunk:?}"));
            let headers = raw_headers
                .lines()
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k, v.trim().to_string()))
                .collect();
            (headers, value.to_string())
        })
        .collect()
}

#[test]
fn a_multipart_request_that_reaches_a_socket_is_parsable() {
    let captured = capture(
        r#"[{"name":"title","value":"报告"},
           {"name":"file","value":"binary-ish","filename":"a.txt"},
           {"name":"note","value":"","contentType":"text/plain"}]"#,
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
    assert_eq!(
        parts.len(),
        3,
        "all three parts must survive the wire: {parts:?}"
    );

    let names: Vec<String> = parts
        .iter()
        .map(|(h, _)| {
            h.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-disposition"))
                .and_then(|(_, v)| {
                    v.split("name=\"")
                        .nth(1)
                        .and_then(|s| s.split('"').next().map(String::from))
                })
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        names,
        vec!["title", "file", "note"],
        "part names and order must be kept"
    );

    // 文件分片带 filename，且缺省 content-type 落到二进制流类型。
    let file_disposition = parts[1]
        .0
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-disposition"))
        .map(|(_, v)| v.clone())
        .expect("file part must carry a Content-Disposition");
    assert!(
        file_disposition.contains(r#"filename="a.txt""#),
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
        "a file part with no explicit content-type must fall back to the binary stream type"
    );

    // 普通字段不该被塞进 Content-Type。
    assert!(
        !parts[0]
            .0
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type")),
        "a plain field must not carry its own Content-Type"
    );

    assert_eq!(
        parts[0].1, "报告",
        "a non-ASCII value must survive byte-for-byte"
    );
    assert_eq!(parts[2].1, "");

    // 收尾分隔符：服务端据此知道没有更多分片了。
    assert!(
        captured
            .body
            .ends_with(&format!("--{}--\r\n", declared_boundary(&captured))),
        "the closing delimiter is missing: {:?}",
        &captured.body[captured.body.len().saturating_sub(48)..]
    );
}

#[test]
fn a_user_set_content_type_is_replaced_but_keeps_its_name() {
    // 这条不走 socket：`apply` 的覆盖规则与传输无关，断言它的输入输出更直接。
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    let assembled = apply(&mut headers, r#"[{"name":"a","value":"1"}]"#);

    let ct_headers: Vec<&(String, String)> = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(
        ct_headers.len(),
        1,
        "a replaced Content-Type must be overwritten in place, not appended: {headers:?}"
    );
    assert_eq!(
        ct_headers[0].0, "content-type",
        "the header name keeps the user's casing"
    );
    let boundary = ct_headers[0]
        .1
        .strip_prefix("multipart/form-data; boundary=")
        .expect("the value must be replaced by the multipart one");
    let body = assembled
        .text
        .expect("a text-only multipart body is valid UTF-8");
    assert!(
        body.contains(&format!("--{boundary}\r\n")),
        "the replacement boundary must be the one in the body"
    );
}

#[test]
fn a_malformed_parts_payload_still_reaches_the_socket_as_valid_multipart() {
    // 零分片仍是格式合法的 body（只有收尾分隔符）。宁可让服务端报「没有分片」，
    // 也不要发一个内容被静默改写的 body。
    let captured = capture("{ not json");
    assert!(
        split_parts(&captured).is_empty(),
        "no part should be invented"
    );
    assert!(
        captured
            .body
            .ends_with(&format!("--{}--\r\n", declared_boundary(&captured))),
        "still a well-formed multipart, just empty"
    );
}
