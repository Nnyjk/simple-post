//! Tauri 2 backend for Simple Post.
//!
//! Responsibilities (this slice):
//!   - Expose the `http_request` command so the React app can issue
//!     real network calls through `reqwest` instead of the browser's
//!     `fetch`. The webview doesn't enforce CORS when the call exits
//!     through a Rust-side HTTP client, which is what fixes the
//!     "Network Error" reported when the app was launched as a plain
//!     Vite SPA.
//!   - Match the contract of `src/lib/http.ts::sendRequest` so the
//!     frontend can swap one transport for the other without changing
//!     call sites.
//!
//! Future slices (not in this file yet): SQLite storage via
//! `tauri-plugin-sql`, MCP server subprocess, native dialogs.

use serde::Serialize;
use std::time::{Duration, Instant};

/// Successful HTTP response, mirrors `ApiResponse` on the TS side.
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
/// which the frontend uses for HTTP-level non-2xx. This is "the request
/// never reached the application layer".
#[derive(Debug, Serialize)]
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
        Self { ok: true, response: Some(r), error: None }
    }
    fn err(kind: &str, msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            response: None,
            error: Some(HttpError { kind: kind.into(), message: msg.into() }),
        }
    }
}

/// Parse the textual HTTP method into a `reqwest::Method`, defaulting
/// to GET on unknown verbs (the frontend already gates on a known list).
fn parse_method(method: &str) -> reqwest::Method {
    match method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => reqwest::Method::from_bytes(other.as_bytes())
            .unwrap_or(reqwest::Method::GET),
    }
}

/// Cap a body string to at most `max_bytes` bytes (UTF-8 safe char
/// boundary). Adds a marker when truncated. Matches the truncation
/// contract from the original fetch path.
fn truncate_body(body: String, max_bytes: u64) -> (String, bool) {
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
    out.push_str(&format!("\n\n[truncated at {} bytes by Tauri HTTP layer]", max_bytes));
    (out, true)
}

/// Issue a real HTTP request through `reqwest`, bypassing browser CORS.
///
/// The frontend calls this when running inside Tauri (where
/// `window.__TAURI_INTERNALS__` is present). When the app is served
/// just by Vite (no Tauri shell), the frontend falls back to the
/// existing `fetch` path so dev iteration on UI doesn't require a
/// Rust rebuild every time.
///
/// NOTE: this function MUST stay non-`pub`. Tauri 2's command +
/// `generate_handler!` macros both emit an internal
/// `__cmd__http_request` macro; emitting them in the same crate-root
/// file with `pub` collides on the macro namespace (E0255). See
/// https://v2.tauri.app/develop/calling-rust/ — "Commands defined in
/// the lib.rs file cannot be marked as pub".
#[tauri::command]
async fn http_request(
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout_ms: u64,
    follow_redirects: bool,
    max_response_size_kb: u64,
) -> HttpResult {
    let start = Instant::now();

    // Per-call Client to keep the API request-scoped; future slices
    // can hoist this into tauri::State if we want connection pooling
    // across calls.
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        })
        .build()
    {
        Ok(c) => c,
        Err(e) => return HttpResult::err("network", format!("client build: {e}")),
    };

    let mut req = client.request(parse_method(&method), &url);
    for (k, v) in headers {
        if !k.is_empty() {
            req = req.header(&k, &v);
        }
    }
    if let Some(b) = body {
        req = req.body(b);
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
        .map(|(k, v)| {
            [k.to_string(), v.to_str().unwrap_or("").to_string()]
        })
        .collect();

    let body_text = match res.text().await {
        Ok(t) => t,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, format!("read body: {e}"));
        }
    };

    let elapsed = start.elapsed().as_millis();
    let max_bytes = max_response_size_kb.saturating_mul(1024);
    let (final_body, truncated) = truncate_body(body_text, max_bytes);

    HttpResult::ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        duration_ms: elapsed,
        size_bytes: final_body.len() as u64,
        headers: header_pairs,
        body: final_body,
        content_type,
        method,
        url,
        truncated,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![http_request])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
