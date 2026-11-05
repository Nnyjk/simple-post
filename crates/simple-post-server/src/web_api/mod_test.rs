//! 主端口的路由集合：**不包含 MCP**。
//!
//! MCP 的端点是守护进程自己的监听端口（见 `mcp_router` 与 `daemon_supervisor`），主端口只
//! 提供 REST 与静态文件。这个文件是那条分界的回归网：
//!
//!   * 主端口上的 `/mcp`、`/mcp/healthz`、`/mcp/tools` 都**不是** MCP 端点；
//!   * 主端口上的 `/api/mcp/*`（状态、日志、生命周期动词）**仍然**可达——那是管理 MCP 的
//!     REST 面，与 MCP 协议本身不是一回事。
//!
//! 断言不能只看状态码：主 router 挂着 SPA fallback，未知路径回的是 `200 text/html` 的壳，
//! 而真的挂上 MCP 路由时回的是 `200 application/json`（`/mcp` 那条只接受 POST，故 GET 回
//! `405`）。状态码分不出「没有这个路由」与「回落的壳」，故这里断言 content-type 与响应体。

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::web_api::{build_router, WebState};

/// SPA 壳里的标记。断言「回的是壳」时用它，而不是断言某个状态码。
const SHELL_MARKER: &str = "<!-- simple-post-spa-shell -->";

/// 起主 router，绑在 loopback 的临时端口上。
///
/// 每个测试的隔离靠原子计数器加 pid，因为 cargo 并行跑测试，好几个会共用同一条临时路径。
async fn serve_main_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir =
        std::env::temp_dir().join(format!("simple-post-web-main-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");
    std::fs::write(
        spa_dir.join("index.html"),
        format!("<!doctype html><html><head>{SHELL_MARKER}</head><body></body></html>"),
    )
    .expect("write shell");

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

/// 主端口上的那三条 MCP 路径都不是 MCP 端点。
#[tokio::test]
async fn the_main_port_does_not_answer_mcp() {
    let (addr, dir) = serve_main_router().await;
    let client = reqwest::Client::new();

    for path in ["/mcp", "/mcp/healthz", "/mcp/tools"] {
        let response = client
            .get(format!("http://{addr}{path}"))
            .send()
            .await
            .expect("request completes");
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await.expect("text body");

        assert_ne!(
            status,
            reqwest::StatusCode::METHOD_NOT_ALLOWED,
            "{path} 上存在一条只接受 POST 的路由 —— 主端口不该有 MCP 端点"
        );
        assert!(
            !content_type.contains("application/json"),
            "{path} 不该回 JSON：{body}"
        );
        assert!(
            !body.contains("registered_tools"),
            "{path} 不该是 MCP 健康探针：{body}"
        );
        assert!(
            !body.contains("serverInfo"),
            "{path} 不该是 MCP JSON-RPC 端点：{body}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `POST /mcp` 在主端口上不受理 JSON-RPC。
#[tokio::test]
async fn the_main_port_does_not_speak_json_rpc() {
    let (addr, dir) = serve_main_router().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .body(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
        )
        .send()
        .await
        .expect("request completes");
    let body = response.text().await.expect("text body");
    assert!(
        !body.contains("serverInfo"),
        "主端口上的 /mcp 不该应答 JSON-RPC：{body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 管理 MCP 的那组 REST 动词仍然在主端口上可达。
///
/// 「主端口不提供 MCP」说的是 MCP 协议本身，不是「管理 MCP 的接口也没了」。
#[tokio::test]
async fn the_admin_surface_for_mcp_still_answers() {
    let (addr, dir) = serve_main_router().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/api/mcp/status"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert!(
        body.get("status").and_then(|v| v.as_str()).is_some(),
        "状态快照要有 status 字段：{body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
