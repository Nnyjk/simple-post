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
///
/// `rename_all = "camelCase"` is load-bearing, not cosmetic: without it
/// serde emits `status_text` / `duration_ms` / `size_bytes` /
/// `content_type` while `src/lib/http.ts` reads `statusText` /
/// `durationMs` / `sizeBytes` / `contentType`. Every one of those came
/// back `undefined`, so the response bar rendered `NaN s` and `0 B` for
/// every request, and the content type never reached the viewer.
/// `HttpResult` below already had the attribute; this type was missed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
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
///
/// `rename_all` is a no-op today (both fields are single words) but keeps
/// this type from re-introducing the mismatch [`HttpResponse`] had.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
// Outbound URL policy.
// -------------------------------------------------------------------------

/// How permissive [`execute`] is about where it is allowed to connect.
///
/// Two independent knobs, because the right answer differs per shape:
///
/// * **Desktop (Tauri)** — the local user *is* the operator. Calling
///   `http://localhost:3000` is the single most common thing an API client
///   does, so blocking private ranges by default would break the product.
///   Only the scheme allowlist applies.
/// * **Web / Docker** — the server is network-reachable and every caller
///   shares one static bearer password. Here the server becomes an open
///   proxy unless private ranges are refused, so operators should set
///   `SIMPLE_POST_BLOCK_PRIVATE_NET=1`.
///
/// `execute` refuses a non-`http(s)` URL under both policies: `file:`,
/// `gopher:` and friends turn a debug tool into a local file reader, and
/// no API client has a legitimate use for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboundPolicy {
    /// Refuse loopback / private / link-local / unique-local addresses.
    /// Driven by `SIMPLE_POST_BLOCK_PRIVATE_NET`.
    pub block_private_net: bool,
}

impl Default for OutboundPolicy {
    /// Desktop-friendly default: scheme allowlist only.
    fn default() -> Self {
        Self {
            block_private_net: false,
        }
    }
}

impl OutboundPolicy {
    /// Read the policy from the environment. Kept here (rather than at
    /// the call sites) so the desktop and web shapes cannot disagree.
    pub fn from_env() -> Self {
        let flag = std::env::var("SIMPLE_POST_BLOCK_PRIVATE_NET")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes" || v == "on"
            })
            .unwrap_or(false);
        Self {
            block_private_net: flag,
        }
    }
}

/// Outcome of checking a target URL against [`OutboundPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlVerdict {
    Allowed,
    /// Human-readable reason the request was refused. Rendered into the
    /// same `HttpResult` envelope as a transport failure, so the UI shows
    /// it with the normal error banner.
    Refused(String),
}

/// True for IPs that must never be reachable through the server-side
/// proxy: loopback, RFC1918, link-local (incl. cloud metadata at
/// 169.254.169.254), carrier-grade NAT, and IPv6 unique-local.
///
/// Hostnames are not resolved here. Resolving would introduce a DNS
/// rebinding window between the check and the dial; a caller that needs
/// name-based policy should be explicit about that tradeoff rather than
/// have a half-measure applied silently.
fn is_forbidden_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // 100.64.0.0/10 — carrier-grade NAT, common in k8s.
                || (v4.octets()[0] == 100
                    && (64..=127).contains(&v4.octets()[1]))
                // 0.0.0.0/8 — "this network", resolves to localhost.
                || v4.octets()[0] == 0
                // 240.0.0.0/4 — reserved, incl. 255.255.255.255.
                || v4.octets()[0] >= 240
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                // fc00::/7 — unique local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 — link local.
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // ::ffff:a.b.c.d — IPv4-mapped; check the embedded v4.
                || v6
                    .to_ipv4_mapped()
                    .map(|v| is_forbidden_ip(std::net::IpAddr::V4(v)))
                    .unwrap_or(false)
        }
    }
}

/// Apply [`OutboundPolicy`] to a target URL string.
pub fn check_url(url: &str, policy: OutboundPolicy) -> UrlVerdict {
    // `reqwest::Url` is a re-export of the `url` crate, so this needs no
    // extra dependency in the workspace.
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        Err(e) => return UrlVerdict::Refused(format!("invalid URL `{url}`: {e}")),
    };

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return UrlVerdict::Refused(format!(
                "scheme `{other}://` is not allowed — only http and https are"
            ))
        }
    }

    if !policy.block_private_net {
        return UrlVerdict::Allowed;
    }

    // `host_str()` keeps IPv6 in `[::1]` form, so strip the brackets
    // before parsing. A name that is not a literal IP is left alone: a
    // hostname is deliberately not resolved here, because resolving would
    // open a DNS-rebinding window between this check and the dial.
    let host_forbidden = parsed
        .host_str()
        .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|h| h.parse::<std::net::IpAddr>().ok())
        .map(is_forbidden_ip)
        .unwrap_or(false);

    if host_forbidden {
        UrlVerdict::Refused(format!(
            "`{url}` resolves to a private, loopback or link-local address, \
             and SIMPLE_POST_BLOCK_PRIVATE_NET is enabled"
        ))
    } else {
        UrlVerdict::Allowed
    }
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
    execute_with_policy(input, OutboundPolicy::from_env()).await
}

/// [`execute`] with an explicit policy. Tests use this directly; the
/// env-driven [`execute`] is the production entry point.
pub async fn execute_with_policy(input: &HttpRequestInput, policy: OutboundPolicy) -> HttpResult {
    let start = Instant::now();

    // Refuse before building a client or touching the network.
    if let UrlVerdict::Refused(reason) = check_url(&input.url, policy) {
        return HttpResult::err("network", reason);
    }

    // Per-call Client to keep the API request-scoped; future slices
    // can hoist this into tauri::State if we want connection pooling
    // across calls.
    //
    // The redirect policy re-runs `check_url` on **every hop**. Checking
    // only the initial URL leaves the whole guard cosmetic: with
    // `follow_redirects` on (the default), an attacker-controlled URL
    // that answers `302 Location: http://169.254.169.254/…` reaches the
    // metadata service in one hop, and `Policy::limited(10)` happily
    // follows it. `custom` gives per-hop control; the hop budget is
    // counted here because `custom` replaces `limited` rather than
    // composing with it.
    const MAX_REDIRECTS: usize = 10;
    let redirect = if input.follow_redirects {
        let hops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        reqwest::redirect::Policy::custom(move |attempt| {
            let n = hops.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match check_url(attempt.url().as_str(), policy) {
                UrlVerdict::Allowed => attempt.follow(),
                UrlVerdict::Refused(reason) => attempt.error(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    reason,
                ))
                    as Box<dyn std::error::Error + Send + Sync>),
            }
        })
    } else {
        reqwest::redirect::Policy::none()
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(input.timeout_ms))
        .redirect(redirect)
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
#[path = "http_executor_test.rs"]
mod tests;
