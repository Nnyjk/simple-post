use super::*;

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
