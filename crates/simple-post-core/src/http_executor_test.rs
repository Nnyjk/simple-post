use super::*;

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

// 线形状

/// `HttpResponse` 必须序列化为 camelCase。
/// 没有 `rename_all` 时 serde 发出 `duration_ms` / `size_bytes` / `content_type` /
/// `status_text`，而 `src/lib/http.ts` 读的是 `durationMs` / `sizeBytes` / `contentType` /
/// `statusText`——每个字段都读回 `undefined`，响应栏于是对每个请求都显示 `NaN s` 与
/// `0 B`。这种不匹配对状态码断言和看起来正常的页面都是隐形的。
#[test]
fn http_response_serialises_camel_case() {
    let r = HttpResponse {
        status: 200,
        status_text: "OK".into(),
        duration_ms: 42,
        size_bytes: 7,
        headers: vec![],
        body: "{}".into(),
        content_type: "application/json".into(),
        method: "GET".into(),
        url: "http://x/y".into(),
        truncated: false,
    };
    let v: serde_json::Value = serde_json::to_value(HttpResult::ok(r)).unwrap();
    let resp = &v["response"];
    for key in [
        "status",
        "statusText",
        "durationMs",
        "sizeBytes",
        "headers",
        "body",
        "contentType",
        "method",
        "url",
        "truncated",
    ] {
        assert!(
            !resp[key].is_null(),
            "`{key}` is missing — the TS side reads camelCase, not snake_case"
        );
    }
    // snake_case 的那些拼法也必须消失。
    for key in ["status_text", "duration_ms", "size_bytes", "content_type"] {
        assert!(
            resp[key].is_null(),
            "`{key}` should not be emitted; it is what the TS side never reads"
        );
    }
}

// 出站 URL 策略

/// 默认策略不得破坏 API 客户端最常做的事：调一个 loopback 上的开发服务器。
#[test]
fn default_policy_allows_loopback_and_private() {
    for url in [
        "http://localhost:3000/api",
        "http://127.0.0.1:8080/healthz",
        "http://192.168.1.10:9000/orders",
        "http://10.0.0.5/internal",
    ] {
        assert_eq!(
            check_url(url, OutboundPolicy::default()),
            UrlVerdict::Allowed,
            "{url} must be allowed under the desktop default"
        );
    }
}

/// scheme 白名单在**两种**策略下都生效。`file://` 会把一个调试工具变成读本地文件的
/// 通道；没有 API 客户端需要它。
#[test]
fn non_http_schemes_are_always_refused() {
    for url in [
        "file:///C:/Windows/win.ini",
        "gopher://127.0.0.1:11211/",
        "ftp://example.com/x",
        "data:text/plain,hello",
    ] {
        assert!(
            matches!(
                check_url(url, OutboundPolicy::default()),
                UrlVerdict::Refused(_)
            ),
            "{url} must be refused even under the permissive default"
        );
        assert!(
            matches!(
                check_url(
                    url,
                    OutboundPolicy {
                        block_private_net: true
                    }
                ),
                UrlVerdict::Refused(_)
            ),
            "{url} must be refused under the strict policy"
        );
    }
}

/// Web 部署的策略恰好拒绝那些让服务端成为 SSRF 支点的网段。`169.254.169.254` 是云
/// 元数据端点。
#[test]
fn strict_policy_refuses_metadata_and_private_ranges() {
    let strict = OutboundPolicy {
        block_private_net: true,
    };
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:9999/admin",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "http://0.0.0.0/",
        "http://100.64.0.1/",
        "http://[::1]/",
        "http://[fd00::1]/",
        "http://[fe80::1]/",
    ] {
        assert!(
            matches!(check_url(url, strict), UrlVerdict::Refused(_)),
            "{url} must be refused when private ranges are blocked"
        );
    }
}

#[test]
fn strict_policy_still_allows_public_addresses() {
    let strict = OutboundPolicy {
        block_private_net: true,
    };
    for url in [
        "https://api.github.com/user",
        "http://93.184.216.34/",
        "https://8.8.8.8/resolve",
    ] {
        assert_eq!(
            check_url(url, strict),
            UrlVerdict::Allowed,
            "{url} is public and must stay reachable"
        );
    }
}

/// 主机名不被解析，故即便在严格策略下，一个看起来公网的名字也被放行。这是被记录在案的
/// 限制而非疏漏——见 `check_url` 上的注释。
#[test]
fn hostname_is_not_resolved_under_strict_policy() {
    let strict = OutboundPolicy {
        block_private_net: true,
    };
    assert_eq!(
        check_url("http://intranet.example.com/api", strict),
        UrlVerdict::Allowed
    );
}

#[test]
fn malformed_url_is_refused() {
    assert!(matches!(
        check_url("not a url", OutboundPolicy::default()),
        UrlVerdict::Refused(_)
    ));
}

#[tokio::test]
async fn execute_refuses_before_dialing() {
    // 绝不能尝试连接：结果是一个拒绝信封，而不是超时。
    let input = HttpRequestInput {
        method: "GET".into(),
        url: "file:///C:/Windows/win.ini".into(),
        headers: vec![],
        body: None,
        timeout_ms: 5_000,
        follow_redirects: true,
        max_response_size_kb: 16,
    };
    let out = execute_with_policy(&input, OutboundPolicy::default()).await;
    assert!(!out.ok);
    let err = out.error.expect("refusal envelope");
    assert_eq!(err.kind, "network");
    assert!(err.message.contains("scheme"), "got: {}", err.message);
}

/// 策略必须挺过一次重定向。
/// 逐跳重检 `check_url` 是承重的：只查初始 URL 时，一个回答 `302` 指向被禁地址的 URL
/// 照样能抵达它——正是这条策略存在的意义所要阻止的威胁。这里起一个真实的 loopback
/// 服务器朝元数据地址回 `302`，并断言那一跳被拒绝。
#[tokio::test]
async fn redirect_hop_is_re_checked_against_the_policy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 302 Found\r\n\
                              Location: http://169.254.169.254/latest/meta-data/\r\n\
                              Content-Length: 0\r\n\r\n",
                    )
                    .await;
            });
        }
    });

    let input = HttpRequestInput {
        method: "GET".into(),
        // 刻意用 `localhost`：策略检查的是 IP 字面量且刻意不解析名字，故**第一跳**被放行，
        // 只有重定向目标才可能被拒。这里若用 `127.0.0.1`，会在初始检查处就被拦下，
        // 对重定向什么也证明不了。
        url: format!("http://localhost:{port}/bounce"),
        headers: vec![],
        body: None,
        timeout_ms: 5_000,
        follow_redirects: true,
        max_response_size_kb: 16,
    };

    let out = execute_with_policy(
        &input,
        OutboundPolicy {
            block_private_net: true,
        },
    )
    .await;
    let err = out.error.expect("redirect must be refused");
    // reqwest 把被拒的一跳报成 "error following redirect"；一个*被跟随*到不可达元数据
    // 地址的跳则会超时并读成 "error sending request"。这个区别正是它成为逐跳重检的
    // 真测试的原因。
    assert!(
        err.message.contains("error following redirect"),
        "expected the redirect to be refused rather than followed, got: {}",
        err.message
    );
}

// parse_method

#[test]
fn parse_method_known_verbs_uppercase() {
    assert_eq!(parse_method("get"), reqwest::Method::GET);
    assert_eq!(parse_method("POST"), reqwest::Method::POST);
    assert_eq!(parse_method("Put"), reqwest::Method::PUT);
    assert_eq!(parse_method("DELETE"), reqwest::Method::DELETE);
    assert_eq!(parse_method("patch"), reqwest::Method::PATCH);
    assert_eq!(parse_method("HEAD"), reqwest::Method::HEAD);
    assert_eq!(parse_method("options"), reqwest::Method::OPTIONS);
}

#[test]
fn parse_method_unparseable_falls_back_to_get() {
    // 防御性的：一个前端 bug 不该让这个 command 500。
    // `from_bytes` 对任何 token 形状的 ASCII 都满意（如 "PROPFIND" 原样通过——见下面的
    // `parse_method_custom_verb_passes_through` 测试），故真正回退到 GET 的只有确实解析
    // 不出的输入（空串，或含空白这类非法字符的 token）。
    assert_eq!(parse_method(""), reqwest::Method::GET);
    assert_eq!(parse_method("BAD METHOD"), reqwest::Method::GET);
}

#[test]
fn parse_method_custom_verb_passes_through() {
    // `from_bytes` 对任何 token 形状的 ASCII 都成功；只在真正解析不出的输入上回退到 GET。
    let m = parse_method("PROPFIND");
    assert_eq!(m, reqwest::Method::from_bytes(b"PROPFIND").unwrap());
}

// truncate_body

#[test]
fn truncate_body_under_cap_is_unchanged() {
    let (out, truncated) = truncate_body("hello".to_string(), 1024);
    assert_eq!(out, "hello");
    assert!(!truncated);
}

#[test]
fn truncate_body_zero_max_means_no_cap() {
    // 0 按文档注释即「功能关闭」；无论多长都原样返回 body。
    let big = "x".repeat(10_000);
    let (out, truncated) = truncate_body(big.clone(), 0);
    assert_eq!(out, big);
    assert!(!truncated);
}

#[test]
fn truncate_body_at_cap_adds_marker() {
    let big = "a".repeat(2000);
    let (out, truncated) = truncate_body(big, 100);
    assert!(truncated);
    assert!(out.contains("[truncated at 100 bytes"));
    // 标记追加在切片之上；切片本身恰好是 `cap` 个 rune。
    assert!(out.starts_with(&"a".repeat(100)));
}

#[test]
fn truncate_body_respects_utf8_char_boundary() {
    // 中日韩字符在 UTF-8 里各 3 字节。在 4 处切会切掉半个 rune——我们期望回退到 3。
    let body = "你好你好".to_string(); // 共 12 字节
    let (out, truncated) = truncate_body(body, 4);
    assert!(truncated);
    // 应当是一个完整的「你」加上标记，而不是半个坏 rune。
    assert!(out.starts_with("你"));
    assert!(out.contains("[truncated"));
    // 没有孤立的续字节（否则会以 U+FFFD 出现）；这里保持简单，只断言切片前缀是合法 UTF-8。
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

#[test]
fn truncate_body_exact_cap_is_not_truncated() {
    // 边界情形：`body.len() <= max_bytes` 是不截断那一支；标记不应出现。
    let body = "abcde".to_string(); // 5 字节
    let (out, truncated) = truncate_body(body, 5);
    assert_eq!(out, "abcde");
    assert!(!truncated);
}

// HttpResult 辅助函数

#[test]
fn http_result_ok_carries_response_only() {
    let r = HttpResult::ok(HttpResponse {
        status: 200,
        status_text: "OK".into(),
        duration_ms: 12,
        size_bytes: 3,
        headers: vec![["content-type".into(), "text/plain".into()]],
        body: "hi!".into(),
        content_type: "text/plain".into(),
        method: "GET".into(),
        url: "http://x".into(),
        truncated: false,
    });
    assert!(r.ok);
    assert!(r.response.is_some());
    assert!(r.error.is_none());
}

#[test]
fn http_result_err_carries_error_only() {
    let r = HttpResult::err("network", "dns blew up");
    assert!(!r.ok);
    assert!(r.response.is_none());
    let err = r.error.unwrap();
    assert_eq!(err.kind, "network");
    assert_eq!(err.message, "dns blew up");
}

// 响应体读取——「读取时护栏」
//
// 下面这几条分别钉住：裁剪结果与整读逐字节一致、上限处的 UTF-8 字符边界回退、
// 带 Content-Length 与 chunked 两条路都在上限处收手。

/// 截断标记文案。它是前后端都看得见的契约（前端响应视图会显示它），故测试直接写死一份，
/// 不从实现里引用——从实现里引用的话，实现改了文案测试也一起变，钉不住任何东西。
const TRUNCATION_MARKER_1KB: &str = "\n\n[truncated at 1024 bytes by Tauri HTTP layer]";

/// 响应头里怎么声明长度。两条路都要覆盖：声明了长度的响应服务端**知道**自己会发多少，
/// chunked 的响应完全不声明——后者只能靠边读边计数来统计长度。
#[derive(Clone, Debug)]
enum BodyFraming {
    /// `Content-Length: N`。
    ContentLength,
    /// `Transfer-Encoding: chunked`。
    Chunked,
}

/// 服务器要送的 body。
#[derive(Clone, Debug)]
enum BodyPlan {
    /// 固定的一份 body，发完就结束响应。
    Fixed { framing: BodyFraming, body: Vec<u8> },
    /// 永远发下去，服务端不结束响应：只有客户端主动收手（断开连接），服务器才会停下来。
    ///
    /// 证据是「客户端在超时之前就拿到了响应」。不用「服务器写出去多少字节」当证据：
    /// Windows 的 TCP 接收缓冲区会自动扩容，字节数那条证据在 Windows 上不成立。
    Endless {
        framing: BodyFraming,
        /// 循环发送的这一块。
        chunk: Vec<u8>,
    },
}

/// 服务器把 body 实际写出去了多少字节，以及它是不是因为「写不出去」而停下。
#[derive(Debug, Clone, Copy)]
struct ServerStats {
    body_bytes_written: u64,
    /// 写不动了（客户端断开或不再收数据）→ 服务器收工。
    stopped: bool,
}

/// 起一个本地 HTTP 服务器：回一份 body，并报告 body 实际写出去了多少字节。
///
/// 用裸 TCP 而不是某个测试框架：http_executor 只跟 reqwest 说话，而这里要控制的恰恰是
/// 「响应头长什么样」与「body 怎么分块送过去」——`Content-Length` 与 chunked 两条路的
/// 差异全在这些细节里，测试框架会把它们藏起来。
///
/// 只统计**第一个**连接的写入量：后来的连接是同一台服务器上另一次请求（例如
/// `legacy_fetch` 对照用的那次），不该混进统计。
async fn spawn_body_server(plan: BodyPlan) -> (u16, oneshot::Receiver<ServerStats>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = oneshot::channel();
    let mut first = Some(tx);
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let plan = plan.clone();
            let reporter = first.take();
            tokio::spawn(async move {
                let mut sock = sock;
                read_request_head(&mut sock).await;
                let stats = write_response(&mut sock, plan).await;
                if let Some(tx) = reporter {
                    let _ = tx.send(stats);
                }
            });
        }
    });
    (port, rx)
}

/// 把请求头读到 `\r\n\r\n` 为止。响应必须等请求头读完再发，否则客户端会先收到响应。
async fn read_request_head(sock: &mut TcpStream) {
    let mut seen: Vec<u8> = Vec::new();
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
        if seen.len() > 64 * 1024 {
            return;
        }
    }
}

/// 带超时的写：客户端不再收数据时，写会一直阻塞而不是报错，故超时也要算「写不出去」。
async fn write_with_timeout(sock: &mut TcpStream, data: &[u8]) -> Result<usize, ()> {
    match tokio::time::timeout(Duration::from_secs(5), sock.write(data)).await {
        Ok(Ok(n)) => Ok(n),
        Ok(Err(_)) => Err(()),
        Err(_) => Err(()),
    }
}

/// 写一个 chunked 块。返回 false 表示写不出去（客户端已经不在了）。
async fn write_chunk(sock: &mut TcpStream, part: &[u8]) -> bool {
    let head = format!("{:x}\r\n", part.len());
    if sock.write_all(head.as_bytes()).await.is_err() {
        return false;
    }
    // 块必须整块写出去：写一半就断会让 chunked 的分帧坏掉，客户端会报协议错误。
    match write_with_timeout(sock, part).await {
        Ok(n) if n == part.len() => {}
        _ => return false,
    }
    sock.write_all(b"\r\n").await.is_ok()
}

/// 写响应头与 body，返回 body 实际写出去了多少字节、以及服务器是不是因为写不出去而停下。
///
/// 写入用 `write` 而不是 `write_all`：客户端在上限处收手并断开连接后，我们需要知道它到底
/// 写出去多少（`write_all` 要么全写完要么什么都不算），否则「客户端提前收手」这条断言就
/// 没有证据。
async fn write_response(sock: &mut TcpStream, plan: BodyPlan) -> ServerStats {
    let head = match &plan {
        BodyPlan::Fixed {
            framing: BodyFraming::ContentLength,
            body,
        } => format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n",
            body.len()
        ),
        BodyPlan::Fixed {
            framing: BodyFraming::Chunked,
            ..
        } => {
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
                .to_string()
        }
        BodyPlan::Endless {
            framing: BodyFraming::ContentLength,
            ..
        } => {
            // 声明一个极大的长度：客户端读到上限就断开，不会注意到这个长度对不上。
            "HTTP/1.1 200 OK\r\nContent-Length: 1073741824\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
                .to_string()
        }
        BodyPlan::Endless {
            framing: BodyFraming::Chunked,
            ..
        } => {
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
                .to_string()
        }
    };
    if sock.write_all(head.as_bytes()).await.is_err() {
        return ServerStats {
            body_bytes_written: 0,
            stopped: true,
        };
    }

    let mut written: u64 = 0;
    let mut stopped = false;
    match plan {
        BodyPlan::Fixed { framing, body } => match framing {
            BodyFraming::ContentLength => {
                while written < body.len() as u64 {
                    match write_with_timeout(sock, &body[written as usize..]).await {
                        Ok(0) | Err(()) => {
                            stopped = true;
                            break;
                        }
                        Ok(n) => written += n as u64,
                    }
                }
            }
            BodyFraming::Chunked => {
                for part in body.chunks(64 * 1024) {
                    if !write_chunk(sock, part).await {
                        stopped = true;
                        break;
                    }
                    written += part.len() as u64;
                }
            }
        },
        BodyPlan::Endless { framing, chunk } => loop {
            match framing {
                BodyFraming::ContentLength => match write_with_timeout(sock, &chunk).await {
                    Ok(0) | Err(()) => {
                        stopped = true;
                        break;
                    }
                    Ok(n) => written += n as u64,
                },
                BodyFraming::Chunked => {
                    if !write_chunk(sock, &chunk).await {
                        stopped = true;
                        break;
                    }
                    written += chunk.len() as u64;
                }
            }
        },
    }
    ServerStats {
        body_bytes_written: written,
        stopped,
    }
}

/// 对照读法：把整个 body 读进内存，再交给 `truncate_body` 裁。
///
/// 测试拿它当基线：流式读取的裁剪结果必须与它逐字节相同。
async fn legacy_fetch(url: &str, max_bytes: u64) -> (String, bool) {
    let res = reqwest::Client::new().get(url).send().await.unwrap();
    let text = res.text().await.unwrap();
    truncate_body(text, max_bytes)
}

/// 流式读取的裁剪结果必须与对照读法（`res.text()` 之后交给 `truncate_body`）逐字节一致。
///
/// 上限之内的内容、UTF-8 字符边界的回退位置、以及 `truncated` 标记，三样都要对上：同一个
/// 响应在桌面形态与 web 形态下必须长得一模一样，而这两条形态走的是同一个
/// `execute_with_policy`。
///
/// 用的样本覆盖了几种容易出错的切点：多字节 rune 正好压在上限上、4 字节的 emoji 压在上限
/// 上、非法 UTF-8 落在上限之前、非法 UTF-8 正好压在上限上、恰好等于上限、只比上限多一个
/// 字节。
#[tokio::test]
async fn streamed_body_matches_legacy_truncation_byte_for_byte() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("纯 ASCII 且远超上限", b"a".repeat(2048)),
        ("多字节 rune 正好压在上限上", {
            // 「你」占 3 字节，从第 1023 字节开始：上限 1024 落在它中间。
            let mut v = b"a".repeat(1023);
            v.extend_from_slice("你好".as_bytes());
            v.extend_from_slice(&b"b".repeat(64));
            v
        }),
        ("4 字节的 emoji 正好压在上限上", {
            // emoji 占 4 字节，从第 1021 字节开始：上限 1024 落在它中间。
            let mut v = b"a".repeat(1021);
            v.extend_from_slice("😀".as_bytes());
            v.extend_from_slice(&b"b".repeat(64));
            v
        }),
        ("非法 UTF-8 落在上限之前", {
            let mut v = b"a".repeat(1000);
            v.extend_from_slice(&[0xFF, 0xFE, 0x80]);
            v.extend_from_slice(&b"b".repeat(200));
            v
        }),
        ("非法 UTF-8 正好压在上限上", {
            let mut v = b"a".repeat(1022);
            v.extend_from_slice(&[0xFF, 0xFF]);
            v.extend_from_slice(&b"b".repeat(200));
            v
        }),
        ("恰好等于上限", b"c".repeat(1024)),
        ("只比上限多一个字节", b"d".repeat(1025)),
    ];

    for (name, body) in cases {
        let (port, _stats) = spawn_body_server(BodyPlan::Fixed {
            framing: BodyFraming::ContentLength,
            body: body.clone(),
        })
        .await;
        let url = format!("http://127.0.0.1:{port}/case");

        let input = HttpRequestInput {
            method: "GET".into(),
            url: url.clone(),
            headers: vec![],
            body: None,
            timeout_ms: 10_000,
            follow_redirects: true,
            max_response_size_kb: 1,
        };
        let out = execute_with_policy(&input, OutboundPolicy::default()).await;
        let resp = out
            .response
            .unwrap_or_else(|| panic!("{name}: 期望拿到一个响应，实际是 {:?}", out.error));

        let legacy = legacy_fetch(&url, 1024).await;
        assert_eq!(
            resp.body, legacy.0,
            "{name}: 裁剪后的 body 与对照读法不一致"
        );
        assert_eq!(
            resp.truncated, legacy.1,
            "{name}: truncated 标记与对照读法不一致"
        );
    }
}

/// 上限处的切点落在一个多字节 rune 中间时，切点必须退到 rune 之前。
///
/// 「你」占 3 字节：从第 1023 字节开始放它，上限 1024 就落在它中间。裁出来的前缀因此是
/// 1023 字节，而不是「1024 字节里夹半个 rune」——半个 rune 在 JSON 里是非法的，前端解析器
/// 会直接报错。
#[tokio::test]
async fn cut_at_the_limit_backs_off_to_the_rune_boundary() {
    let mut payload = b"a".repeat(1023);
    payload.extend_from_slice("你好".as_bytes());
    payload.extend_from_slice(&b"b".repeat(64));

    let (port, _stats) = spawn_body_server(BodyPlan::Fixed {
        framing: BodyFraming::ContentLength,
        body: payload,
    })
    .await;
    let input = HttpRequestInput {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/rune"),
        headers: vec![],
        body: None,
        timeout_ms: 10_000,
        follow_redirects: true,
        max_response_size_kb: 1,
    };
    let out = execute_with_policy(&input, OutboundPolicy::default()).await;
    let resp = out.response.expect("期望拿到一个响应");

    assert!(
        resp.truncated,
        "1024 字节上限遇到 1087 字节的 body 必须被标记为已截断"
    );
    let prefix = resp
        .body
        .strip_suffix(TRUNCATION_MARKER_1KB)
        .expect("裁剪后的 body 必须以截断标记结尾");
    assert_eq!(
        prefix.len(),
        1023,
        "切点必须退到「你」之前，而不是切在它中间"
    );
    assert!(prefix.ends_with('a'), "前缀应停在 ASCII 上");
    assert!(
        std::str::from_utf8(prefix.as_bytes()).is_ok(),
        "前缀必须是合法 UTF-8，不能含半个 rune"
    );
}

/// 恰好等于上限的响应体不裁：这条边界决定了 `truncated` 标记在多长时出现。
#[tokio::test]
async fn body_exactly_at_the_limit_is_not_truncated() {
    let payload = b"e".repeat(1024);
    let (port, _stats) = spawn_body_server(BodyPlan::Fixed {
        framing: BodyFraming::ContentLength,
        body: payload.clone(),
    })
    .await;
    let input = HttpRequestInput {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/exact"),
        headers: vec![],
        body: None,
        timeout_ms: 10_000,
        follow_redirects: true,
        max_response_size_kb: 1,
    };
    let out = execute_with_policy(&input, OutboundPolicy::default()).await;
    let resp = out.response.expect("期望拿到一个响应");

    assert!(!resp.truncated, "恰好 1024 字节不应被标记为已截断");
    assert_eq!(resp.body, String::from_utf8_lossy(&payload));
    assert!(!resp.body.contains("[truncated"), "不应出现截断标记");
}

/// 带 `Content-Length` 的超大响应：客户端必须在上限处收手，不能把整个 body 收进内存。
///
/// 服务端**不结束**响应（一直发下去），于是「客户端收手了」有了一个不依赖平台行为的证据：
/// 客户端在请求超时（3 秒）之前就拿到了响应。
///
/// 请求超时故意设成 3 秒：它比「读完上限」所需的时间长得多，却比测试愿意等的时间短。
#[tokio::test]
async fn oversized_content_length_response_stops_reading_at_the_limit() {
    let chunk = b"a".repeat(64 * 1024);
    let (port, stats_rx) = spawn_body_server(BodyPlan::Endless {
        framing: BodyFraming::ContentLength,
        chunk: chunk.clone(),
    })
    .await;
    let input = HttpRequestInput {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/big"),
        headers: vec![],
        body: None,
        timeout_ms: 3_000,
        follow_redirects: true,
        max_response_size_kb: 1,
    };
    let out = execute_with_policy(&input, OutboundPolicy::default()).await;
    let resp = out
        .response
        .unwrap_or_else(|| panic!("客户端应当在上限处收手，而不是读到超时：{:?}", out.error));

    assert!(resp.truncated, "超出上限的响应必须被标记为已截断");
    let expected_head = "a".repeat(1024);
    assert!(
        resp.body.starts_with(&expected_head),
        "裁出来的前缀必须是响应的前 1024 字节"
    );
    assert!(resp.body.contains(TRUNCATION_MARKER_1KB));

    // 服务端最终会因为写不出去而停下：客户端断开连接了。
    let stats = tokio::time::timeout(Duration::from_secs(20), stats_rx)
        .await
        .expect("服务器任务应当结束")
        .expect("服务器应当报告写入量");
    assert!(
        stats.stopped,
        "服务器写出了 {} 字节却没有停下——客户端还在收数据",
        stats.body_bytes_written
    );
}

/// chunked 的超大响应：服务端不声明长度，收手的判据只能是边读边数的计数。
#[tokio::test]
async fn oversized_chunked_response_stops_reading_at_the_limit() {
    let chunk = b"a".repeat(64 * 1024);
    let (port, stats_rx) = spawn_body_server(BodyPlan::Endless {
        framing: BodyFraming::Chunked,
        chunk: chunk.clone(),
    })
    .await;
    let input = HttpRequestInput {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/chunked"),
        headers: vec![],
        body: None,
        timeout_ms: 3_000,
        follow_redirects: true,
        max_response_size_kb: 1,
    };
    let out = execute_with_policy(&input, OutboundPolicy::default()).await;
    let resp = out
        .response
        .unwrap_or_else(|| panic!("客户端应当在上限处收手，而不是读到超时：{:?}", out.error));

    assert!(resp.truncated, "超出上限的 chunked 响应必须被标记为已截断");
    let expected_head = "a".repeat(1024);
    assert!(
        resp.body.starts_with(&expected_head),
        "裁出来的前缀必须是响应的前 1024 字节"
    );
    assert!(resp.body.contains(TRUNCATION_MARKER_1KB));

    let stats = tokio::time::timeout(Duration::from_secs(20), stats_rx)
        .await
        .expect("服务器任务应当结束")
        .expect("服务器应当报告写入量");
    assert!(
        stats.stopped,
        "服务器写出了 {} 字节却没有停下——客户端还在收数据",
        stats.body_bytes_written
    );
}
