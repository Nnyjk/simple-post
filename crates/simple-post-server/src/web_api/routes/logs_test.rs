//! `GET /api/logs/export` 与 `POST /api/logs/client` 的线上形状。
//!
//! 缓冲区是进程级全局状态，故每条用例都用独有的标记串认自己的记录，不对整份文本的
//! 长度做断言。

use std::net::SocketAddr;
use std::path::PathBuf;

use simple_post_core::logbuf;

use crate::web_api::{build_router, WebState};

/// 一遍性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 的临时端口上。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir =
        std::env::temp_dir().join(format!("simple-post-web-logs-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");

    let pool = crate::db::init(&dir).expect("db init");
    let state = WebState {
        pool,
        daemon: crate::daemon_supervisor::McpDaemonHandle::new(),
        data_dir: dir.clone(),
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

/// 缺省 `scope` 按 `all` 处理，字段是 camelCase 的 `fileName` / `content`。
#[tokio::test]
async fn the_export_route_returns_camel_case_fields_and_defaults_to_all() {
    logbuf::record(
        simple_post_core::logbuf::LogLevel::Info,
        "web::logs",
        "route-all-marker",
    );
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/api/logs/export"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");

    assert!(
        body["fileName"]
            .as_str()
            .expect("fileName is a string")
            .starts_with("simple-post-logs-"),
        "缺省范围应导全部日志，got: {body}"
    );
    assert!(
        body["content"]
            .as_str()
            .expect("content is a string")
            .contains("route-all-marker"),
        "缺省范围应包含 Info 级记录，got: {body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `scope=errors` 只回 Warn 及以上，且文件名指向错误日志。
#[tokio::test]
async fn the_errors_scope_drops_info_lines() {
    logbuf::record(
        simple_post_core::logbuf::LogLevel::Info,
        "web::logs",
        "route-info-marker",
    );
    logbuf::record(
        simple_post_core::logbuf::LogLevel::Warn,
        "web::logs",
        "route-warn-marker",
    );
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/api/logs/export?scope=errors"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");

    assert!(
        body["fileName"]
            .as_str()
            .expect("fileName is a string")
            .starts_with("simple-post-errors-"),
        "got: {body}"
    );
    let content = body["content"].as_str().expect("content is a string");
    assert!(content.contains("route-warn-marker"));
    assert!(
        !content.contains("route-info-marker"),
        "errors 范围不该带上 Info 级记录"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 未知 `scope` 是 400，不是静默按 `all`。
#[tokio::test]
async fn an_unknown_scope_is_rejected() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/api/logs/export?scope=warnings"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 渲染层上报回 204，且确实进了缓冲区、挂在自己的 `webview:<source>` 名下。
#[tokio::test]
async fn a_client_event_round_trips_through_the_route() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{addr}/api/logs/client"))
        .json(&serde_json::json!({
            "level": "error",
            "source": "route-smoke",
            "message": "route-client-marker",
            "detail": "stack",
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:route-smoke")
        .expect("上报应当写进缓冲区");
    assert_eq!(hit.message, "route-client-marker\nstack");

    let _ = std::fs::remove_dir_all(&dir);
}
