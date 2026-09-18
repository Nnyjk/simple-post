//! HTTP execution layer.
//!
//! Pure I/O — no business logic. The caller (a Tauri command, the MCP
//! daemon, or a future cron worker) builds an [`HttpRequestInput`],
//! hands it to [`execute`], and gets back an [`HttpResult`] shaped to
//! match the wire contract the React renderer already understands
//! (see `src/lib/http.ts::ApiResponse`).
//!
//! This module is the only place in the backend that talks to
//! `reqwest`. Splitting the wire types and the actual network call
//! out of `lib.rs` makes the layer reusable from the MCP daemon
//! without dragging in the Tauri `#[command]` macro plumbing (and
//! the macro-namespace collision that comes with it — see the
//! `http_request` NOTE in `lib.rs`).

use std::time::{Duration, Instant};

use serde::Serialize;

// -------------------------------------------------------------------------
// Wire types — mirror `ApiResponse` / `HttpError` on the TS side.
// -------------------------------------------------------------------------

/// Successful HTTP response. Field names match `ApiResponse` so the
/// React side can reuse its existing response-view code without a
/// translation layer.
#[derive(Debug, Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u128,
    pub size_bytes: u64,
    /// Flattened headers as `[name, value]` pairs (matches the
    /// `headersToObject` shape on the JS side).
    pub headers: Vec<[String; 2]>,
    pub body: String,
    pub content_type: String,
    pub method: String,
    pub url: String,
    /// True when the response body was clamped to `max_bytes` server-side.
    pub truncated: bool,
}

/// Transport-level failure. Distinct from `HttpResponse.status == 0`,
/// which the frontend uses for HTTP-level non-2xx. This is "the
/// request never reached the application layer" — DNS, connect,
/// TLS, reset, aborted, timeout.
#[derive(Debug, Clone, Serialize)]
pub struct HttpError {
    /// `timeout` | `network` (DNS / connect / TLS / reset / aborted)
    pub kind: String,
    pub message: String,
}

/// Discriminated union — easier to consume from TS than a bare enum
/// because the field shape is stable regardless of variant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<HttpResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<HttpError>,
}

impl HttpResult {
    fn ok(r: HttpResponse) -> Self {
        Self {
            ok: true,
            response: Some(r),
            error: None,
        }
    }
    fn err(kind: &str, msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            response: None,
            error: Some(HttpError {
                kind: kind.into(),
                message: msg.into(),
            }),
        }
    }
}

// -------------------------------------------------------------------------
// Input — the thing the caller builds before handing off.
// -------------------------------------------------------------------------

/// Everything `execute` needs to know about one request. Kept as a
/// flat struct (not a builder) because every caller already knows
/// all the fields up front — the build logic lives in
/// `crate::variable_fill`, not here.
#[derive(Debug, Clone)]
pub struct HttpRequestInput {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

// -------------------------------------------------------------------------
// Internals — small enough that inlining them as `pub(crate)` keeps
// the public surface clean. The Tauri command in lib.rs and the
// eventual MCP daemon both call `execute`; nothing else needs to
// touch these.
// -------------------------------------------------------------------------

/// Parse the textual HTTP method into a `reqwest::Method`, defaulting
/// to GET on unknown verbs (the frontend already gates on a known
/// list).
pub(crate) fn parse_method(method: &str) -> reqwest::Method {
    match method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => reqwest::Method::from_bytes(other.as_bytes()).unwrap_or(reqwest::Method::GET),
    }
}

/// Cap a body string to at most `max_bytes` bytes (UTF-8 safe char
/// boundary). Adds a marker when truncated. Matches the truncation
/// contract from the original fetch path.
pub(crate) fn truncate_body(body: String, max_bytes: u64) -> (String, bool) {
    if max_bytes == 0 {
        return (body, false);
    }
    if body.len() as u64 <= max_bytes {
        return (body, false);
    }
    let cap = (max_bytes as usize).min(body.len());
    // Walk back to the nearest char boundary so we don't slice a
    // multi-byte rune in half.
    let mut cut = cap;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 96);
    out.push_str(&body[..cut]);
    out.push_str(&format!(
        "\n\n[truncated at {} bytes by Tauri HTTP layer]",
        max_bytes
    ));
    (out, true)
}

// -------------------------------------------------------------------------
// Public entry — the actual HTTP call.
// -------------------------------------------------------------------------

/// Issue a real HTTP request through `reqwest`, bypassing browser
/// CORS. Pure async I/O; no shared state.
///
/// The frontend calls the `http_request` Tauri command in `lib.rs`,
/// which forwards here. The MCP daemon (`crate::mcp`) routes
/// requests through its own `execute_http` shim today and will
/// eventually call this directly when its compat layer is retired.
pub async fn execute(input: &HttpRequestInput) -> HttpResult {
    let start = Instant::now();

    // Per-call Client to keep the API request-scoped; future slices
    // can hoist this into tauri::State if we want connection pooling
    // across calls.
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(input.timeout_ms))
        .redirect(if input.follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        })
        .build()
    {
        Ok(c) => c,
        Err(e) => return HttpResult::err("network", format!("client build: {e}")),
    };

    let mut req = client.request(parse_method(&input.method), &input.url);
    for (k, v) in &input.headers {
        if !k.is_empty() {
            req = req.header(k, v);
        }
    }
    if let Some(ref b) = input.body {
        req = req.body(b.clone());
    }

    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, e.to_string());
        }
    };

    let status = res.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let header_pairs: Vec<[String; 2]> = res
        .headers()
        .iter()
        .map(|(k, v)| [k.to_string(), v.to_str().unwrap_or("").to_string()])
        .collect();

    let body_text = match res.text().await {
        Ok(t) => t,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, format!("read body: {e}"));
        }
    };

    let elapsed = start.elapsed().as_millis();
    let max_bytes = input.max_response_size_kb.saturating_mul(1024);
    let (final_body, truncated) = truncate_body(body_text, max_bytes);

    HttpResult::ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        duration_ms: elapsed,
        size_bytes: final_body.len() as u64,
        headers: header_pairs,
        body: final_body,
        content_type,
        method: input.method.clone(),
        url: input.url.clone(),
        truncated,
    })
}

// -------------------------------------------------------------------------
// Tests — internal helpers + the result envelope.
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
}