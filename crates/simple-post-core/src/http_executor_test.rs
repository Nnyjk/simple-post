use super::*;

// ---- wire shape -------------------------------------------------------

/// `HttpResponse` must serialise in camelCase.
///
/// Regression net for a real UI defect: the type had no
/// `rename_all`, so serde emitted `duration_ms` / `size_bytes` /
/// `content_type` / `status_text` while `src/lib/http.ts` read
/// `durationMs` / `sizeBytes` / `contentType` / `statusText`. Every
/// field came back `undefined`, so the response bar showed
/// `NaN s` and `0 B` for every single request. The mismatch is
/// invisible to a status-code assertion and to a live-looking page,
/// which is why it survived; only a browser read showed it.
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
    // And the snake_case spellings must be gone.
    for key in ["status_text", "duration_ms", "size_bytes", "content_type"] {
        assert!(
            resp[key].is_null(),
            "`{key}` should not be emitted; it is what the TS side never reads"
        );
    }
}

// ---- outbound URL policy ----------------------------------------------
/// The default policy must not break the most common thing an API
/// client does: calling a dev server on loopback.
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

/// Scheme allowlist applies under BOTH policies. `file://` turns a
/// debug tool into a local file reader; no API client needs it.
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

/// The web-deployment policy refuses exactly the ranges that make the
/// server an SSRF pivot. `169.254.169.254` is the cloud metadata
/// endpoint and was a live open proxy before this check existed.
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

/// A hostname is not resolved, so a public-looking name is allowed
/// even under the strict policy. Documented limitation, not an
/// oversight — see the comment on `check_url`.
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
    // Must not attempt the connection: the result is a refusal
    // envelope, not a timeout.
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

/// The policy must survive a redirect.
///
/// Regression net for a guard that was purely cosmetic: `check_url`
/// ran on the initial URL only, while the client was built with
/// `Policy::limited(10)`. One `302` to a forbidden address therefore
/// reached it — exactly the threat the policy exists to stop. Spins a
/// real loopback server that answers `302` toward the metadata
/// address and asserts the hop is refused.
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
        // `localhost` on purpose: the policy checks IP literals and
        // deliberately does not resolve names, so the *first* hop is
        // allowed and only the redirect target can be refused. Using
        // `127.0.0.1` here would be stopped at the initial check and
        // would prove nothing about redirects.
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
    // reqwest surfaces a refused hop as "error following redirect";
    // a *followed* hop to an unreachable metadata address would instead
    // time out and read "error sending request". The distinction is
    // what makes this a real test of the per-hop re-check.
    assert!(
        err.message.contains("error following redirect"),
        "expected the redirect to be refused rather than followed, got: {}",
        err.message
    );
}

// ---- parse_method -----------------------------------------------------

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
    // Defensive: a frontend bug shouldn't 500 the command.
    //
    // `from_bytes` is happy with any token-shape ASCII (e.g. "PROPFIND"
    // or "FROBNICATE" passes through unchanged — see the
    // `parse_method_custom_verb_passes_through` test below), so the only
    // inputs that actually fall back to GET are the genuinely unparseable
    // ones (empty string, or tokens containing illegal characters such as
    // whitespace).
    assert_eq!(parse_method(""), reqwest::Method::GET);
    assert_eq!(parse_method("BAD METHOD"), reqwest::Method::GET);
}

#[test]
fn parse_method_custom_verb_passes_through() {
    // `from_bytes` succeeds for any token-shape ASCII; we only fall
    // back to GET on truly unparseable input.
    let m = parse_method("PROPFIND");
    assert_eq!(m, reqwest::Method::from_bytes(b"PROPFIND").unwrap());
}

// ---- truncate_body ----------------------------------------------------

#[test]
fn truncate_body_under_cap_is_unchanged() {
    let (out, truncated) = truncate_body("hello".to_string(), 1024);
    assert_eq!(out, "hello");
    assert!(!truncated);
}

#[test]
fn truncate_body_zero_max_means_no_cap() {
    // 0 = "feature disabled" per the docstring; the body is
    // returned verbatim regardless of length.
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
    // Marker is appended on top of the slice; the slice itself is
    // exactly `cap` runes.
    assert!(out.starts_with(&"a".repeat(100)));
}

#[test]
fn truncate_body_respects_utf8_char_boundary() {
    // Each Chinese character is 3 bytes in UTF-8. Cutting at 4
    // would slice a rune — we expect to walk back to 3.
    let body = "你好你好".to_string(); // 12 bytes total
    let (out, truncated) = truncate_body(body, 4);
    assert!(truncated);
    // We should have one full "你" + marker, NOT a broken rune.
    assert!(out.starts_with("你"));
    assert!(out.contains("[truncated"));
    // No lone continuation bytes (replacement would have shown up
    // as the U+FFFD char; we keep things simple and just assert
    // the slice prefix is valid UTF-8).
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

#[test]
fn truncate_body_exact_cap_is_not_truncated() {
    // Boundary case: `body.len() <= max_bytes` is the no-truncation
    // branch; the marker must not appear.
    let body = "abcde".to_string(); // 5 bytes
    let (out, truncated) = truncate_body(body, 5);
    assert_eq!(out, "abcde");
    assert!(!truncated);
}

// ---- HttpResult helpers -----------------------------------------------

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
