// crates/simple-post-mcp/src/http_client.rs
//
// 访问运行中的 `simple-post serve-web` 的 `reqwest::Client` 薄封装，
// 持有 bearer 密码，调用方不必逐个方法往下传。
//
// 方法一律 `async fn`：lib 已经通过 `simple-post-core::http_executor`
// 带进了异步 client，没有开启 `reqwest::blocking`。

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
    /// `base_url` 写 `http://host:port`（尾斜杠内部会 trim）；
    /// `password` 来自 `--password` / `SIMPLE_POST_WEB_PASSWORD`。
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

    /// POST 一帧 JSON-RPC 到 `/mcp`，原样返回 body；解析交给调用方
    /// （stdio_loop）。
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

    /// 存活探测 —— GET `/mcp/healthz`，返回解析后的 JSON。供
    /// `--transport=http` 起一个最小 HTTP 面给 curl 检查。
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

/// 把 `ClientError` 折成 stdio loop 写到 stdout 的 JSON-RPC `error` 块。
/// 401/403 → -32001（鉴权）；其余 4xx/5xx → -32000；其他 → -32603。
pub fn to_rpc_error(err: ClientError) -> (i64, String) {
    match &err {
        ClientError::Backend { status, body } => (http_status_to_rpc_code(*status), body.clone()),
        ClientError::Http(msg) => (-32603, format!("transport: {msg}")),
        ClientError::Stdio(msg) => (-32603, format!("stdio: {msg}")),
        ClientError::BadFrame(msg) => (-32700, format!("parse error: {msg}")),
    }
}

/// 状态码分类，决定是交给 `to_rpc_error` 还是当瞬时失败让调用方重试。
pub fn is_auth_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 401 | 403)
}
