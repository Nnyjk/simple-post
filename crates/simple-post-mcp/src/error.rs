// crates/simple-post-mcp/src/error.rs
//
// Error types for the standalone `simple-post-mcp` client. We never
// surface Rust errors verbatim on stdout because the JSON-RPC channel
// is line-oriented text — a panic message or a multi-line backtrace
// would corrupt the wire format and hang the AI client.
//
// `map_http_status` produces the JSON-RPC error code we hand back when
// the web backend rejects a request:
//   - HTTP 401 (unauthenticated / wrong password) → -32001
//   - other 4xx / 5xx                                → -32000
//   - transport-level failure (connect / read)        → -32603 (internal)

use simple_post_core::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP transport failure: {0}")]
    Http(String),

    #[error("I/O failure on stdio: {0}")]
    Stdio(String),

    #[error("invalid JSON-RPC frame: {0}")]
    BadFrame(String),

    #[error("backend returned status {status}: {body}")]
    Backend { status: u16, body: String },
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Http(e.to_string())
    }
}

/// Convert a `(status, body)` pair from the web backend into the
/// JSON-RPC `code` field. The convention here matches what the
/// existing `simple-post-mcp-proxy` shipped (commit 0a) so AI clients
/// see the same code on either binary.
pub fn http_status_to_rpc_code(status: u16) -> i64 {
    if status == 401 || status == 403 {
        // Wrong / missing password. Distinct from "backend error" so
        // AI clients can decide whether to retry with a new token.
        -32001
    } else if status >= 400 {
        // Generic backend rejection.
        -32000
    } else {
        // 2xx / 3xx shouldn't reach this path because the caller
        // checks status before mapping, but keep it well-defined.
        0
    }
}

impl From<AppError> for ClientError {
    fn from(_e: AppError) -> Self {
        // We don't try to translate every AppError variant — the
        // standalone client only talks to `/mcp` and `/mcp/healthz`
        // over HTTP, so AppError only ever appears wrapped in an
        // HTTP response body. The `Backend` arm surfaces it.
        ClientError::BadFrame("AppError in standalone pipeline".into())
    }
}