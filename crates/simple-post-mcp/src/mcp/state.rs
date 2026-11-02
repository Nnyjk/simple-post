//! daemon 共享状态与启动入口。

use std::sync::Arc;
use std::time::Instant;

/// 每个 axum handler 共享的 daemon 状态。
pub struct AppState {
    /// DB 连接池。`r2d2::Pool` clone 廉价（内部是 Arc），`AppState` 外面还包了一层
    /// Arc，router 每个 handler 拿的是那一份的 clone；真正访问时再 clone 一次池。
    pub pool: simple_post_core::db::DbPool,
    /// uptime 报告的墙钟起点。
    pub started_at: Instant,
    /// 实时计数器，每处理一个请求就自增，供 `/mcp/healthz` 与 `mcp_status` 展示。
    pub metrics: Arc<simple_post_core::mcp_metrics::McpMetrics>,
}

/// 在 `bind:port` 上起 MCP HTTP daemon，返回**实际**端口。
///
/// `port = 0` 表示交给 OS 分配（测试与写端口文件的 dev supervisor 需要）。
pub async fn serve_http(
    pool: simple_post_core::db::DbPool,
    port: u16,
    bind: &str,
) -> Result<u16, simple_post_core::error::AppError> {
    let listener = tokio::net::TcpListener::bind((bind, port))
        .await
        .map_err(|e| simple_post_core::error::AppError::Http(format!("bind {bind}:{port}: {e}")))?;
    let actual_port = listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| simple_post_core::error::AppError::Http(format!("local_addr: {e}")))?;

    let state = Arc::new(AppState {
        pool,
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    });

    tokio::spawn(async move {
        if let Err(e) = crate::daemon_http::serve(listener, state).await {
            tracing::error!(error = %e, "MCP daemon exited with error");
        }
    });

    Ok(actual_port)
}
