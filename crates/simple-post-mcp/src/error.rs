// crates/simple-post-mcp/src/error.rs
//
// 独立客户端的错误类型。Rust 错误原文绝不直接吐到 stdout：JSON-RPC 通道是
// 逐行文本，一段 panic message 或多行 backtrace 就会破坏帧格式并挂住客户端。
//
// `http_status_to_rpc_code` 给出后端拒绝时回传的 JSON-RPC code：
//   - HTTP 401/403（密码错或缺失）      → -32001
//   - 其余 4xx / 5xx                     → -32000
//   - 传输层失败（连接 / 读）            → -32603 (internal)

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

/// 把后端返回的 `(status, body)` 折成 JSON-RPC `code`，让换哪个二进制都能给
/// AI 客户端同样的 code。
pub fn http_status_to_rpc_code(status: u16) -> i64 {
    if status == 401 || status == 403 {
        // 密码错/缺失。与「后端出错」分开，客户端才知道该不该换个 token 重试。
        -32001
    } else if status >= 400 {
        // 其余 4xx / 5xx 一律归为后端拒绝。
        -32000
    } else {
        // 调用方先判过状态码，2xx / 3xx 到不了这里；仍给出确定取值。
        0
    }
}

impl From<AppError> for ClientError {
    fn from(_e: AppError) -> Self {
        // 不逐个 variant 翻译：独立客户端只经 HTTP 访问 `/mcp` 与 `/mcp/healthz`，
        // `AppError` 只会裹在响应体里，由 `Backend` 分支透出。
        ClientError::BadFrame("AppError in standalone pipeline".into())
    }
}
