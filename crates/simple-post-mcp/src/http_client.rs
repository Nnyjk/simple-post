// crates/simple-post-mcp/src/http_client.rs
//
// Thin wrapper over `reqwest::Client` for talking to a running
// `simple-post serve-web` instance. Holds the bearer password (if
// any) so callers don't have to thread it through every method.
//
// All methods are synchronous because the stdio loop runs on a
// `spawn_blocking` task (see `main.rs`); the `reqwest::blocking`
// feature isn't pulled in (the lib already has the async client via
// `simple-post-core::http_executor`), so we use the async client and
// `tokio::task::block_in_place` from the caller side. If we ever
// need true async, swap this for `async fn`.

use reqwest::{header, Client, StatusCode};
use serde_json::Value;

use crate::error::{http_status_to_rpc_code, ClientError};

#[derive(Debug, Clone)]
pub struct WebClient {
    base_url: String,
    password: Option<String>,
    inner: Client,
}

impl WebClient {
    /// Build a client. `base_url` should be `http://host:port` (no
    /// trailing slash). `password` is the optional bearer token
    /// passed as `--password` or `SIMPLE_POST_WEB_PASSWORD`.
    pub fn new(base_url: impl Into<String>, password: Option<String>) -> Self {
        let inner = Client::builder()
            .pool_max_idle_per_host(4)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest::Client builder");
        Self {
            base_url: base_url.into(),
            password,
            inner,
        }
    }

    /// POST a JSON-RPC frame to `/mcp` and return the raw response
    /// body. Caller (stdio_loop) is responsible for parsing.
    pub async fn post_mcp(&self, frame: &Value) -> Result<Value, ClientError> {
        let url = format!("{}/mcp", self.base_url.trim_end_matches('/'));
        let mut req = self.inner.post(&url);
        if let Some(token) = self.password.as_deref() {
            req = req.bearer_auth(token);
        }
        let resp = req
            .header(header::CONTENT_TYPE, "application/json")
            .json(frame)
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            Ok(body)
        } else {
            Err(ClientError::Backend {
                status: status.as_u16(),
                body: body.to_string(),
            })
        }
    }

    /// Liveness probe — GET `/mcp/healthz`. Returns the parsed JSON
    /// or an error string. Used by `--transport=http` to surface a
    /// minimal HTTP surface for curl checks.
    pub async fn healthz(&self) -> Result<Value, ClientError> {
        let url = format!("{}/mcp/healthz", self.base_url.trim_end_matches('/'));
        let mut req = self.inner.get(&url);
        if let Some(token) = self.password.as_deref() {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            Ok(body)
        } else {
            Err(ClientError::Backend {
                status: status.as_u16(),
                body: body.to_string(),
            })
        }
    }
}

/// Map a `ClientError` into the JSON-RPC `error` block the stdio
/// loop writes to stdout. HTTP 401/403 → -32001 (auth); other HTTP
/// 4xx/5xx → -32000; everything else → -32603 (internal).
pub fn to_rpc_error(err: ClientError) -> (i64, String) {
    match &err {
        ClientError::Backend { status, body } => (http_status_to_rpc_code(*status), body.clone()),
        ClientError::Http(msg) => (-32603, format!("transport: {msg}")),
        ClientError::Stdio(msg) => (-32603, format!("stdio: {msg}")),
        ClientError::BadFrame(msg) => (-32700, format!("parse error: {msg}")),
    }
}

/// Convenience: classify a status code so we can decide whether to
/// hand it to `to_rpc_error` or treat it as a transient failure that
/// the caller should retry.
pub fn is_auth_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 401 | 403)
}
