//! MCP daemon 日志路由的路由与线格式。
//!
//! 这条路由是设置页日志区的唯一数据来源。它得先**可达**（不落到 SPA fallback 的
//! `404`），再把线格式说对——`capturing` 那个字段尤其重要：复用一个外部启动的
//! daemon 时没有 stderr 管道，那种情况下前端要显示「无日志捕获」而不是「没有日志」。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
///
/// 每个测试的隔离靠原子计数器加 pid，因为 cargo 并行跑测试，好几个会共用同一条临时路径。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-mcp-routing-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");

    let pool = crate::db::init(&dir).expect("db init");
    let state = WebState {
        pool: pool.clone(),
        daemon: crate::daemon_supervisor::McpDaemonHandle::new(),
        data_dir: dir.clone(),
        mcp_state: Arc::new(crate::mcp::AppState {
            pool,
            started_at: std::time::Instant::now(),
            metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
        }),
        password: None,
        base_path: None,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let router = build_router(state, spa_dir);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 日志路由可达，且在没有 daemon 被拉起时如实报告「无日志捕获」。
#[tokio::test]
async fn the_log_route_answers_and_says_it_captures_nothing() {
    let (addr, dir) = serve_test_router().await;

    let response = reqwest::Client::new()
        .get(format!("http://{addr}/api/mcp/logs"))
        .send()
        .await
        .expect("request completes");

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "expected the log handler to run (200), not a routing failure"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["lines"], serde_json::json!([]));
    assert_eq!(
        body["capturing"], false,
        "没有 daemon 被拉起就没有 stderr 管道"
    );
    assert_eq!(body["truncated"], false);
    // 分界缺席即「没有本次运行」——它有值才谈得上重启的分界。
    assert!(body.get("startedAtMs").is_none() || body["startedAtMs"].is_null());

    let _ = std::fs::remove_dir_all(&dir);
}
