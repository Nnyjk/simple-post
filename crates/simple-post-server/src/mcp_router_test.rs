//! MCP 守护进程那个端口上的路由集合。
//!
//! 四件事要钉住：
//!
//!   * MCP 的三条路由（JSON-RPC、健康探针、工具清单）在那个端口上照常工作；
//!   * bearer 口令拦住没有令牌的请求，而健康探针与公开名单里的路径不受影响；
//!   * 配置里的挂载路径生效，`/mcp` 仍作为别名可用；
//!   * Host 白名单在那个端口上有约束力（那个端口不是绕过白名单的后门）。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::McpHttpConfig;
use crate::mcp_router::build_mcp_router;

/// 起一个 MCP 守护进程的 router，绑在 loopback 的临时端口上。
///
/// 每个测试的隔离靠原子计数器加 pid，因为 cargo 并行跑测试，好几个会共用同一条临时路径。
async fn serve_mcp_router(config: McpHttpConfig, password: Option<&str>) -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-router-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let pool = crate::db::init(&dir).expect("db init");
    let state = Arc::new(crate::mcp::AppState {
        pool,
        started_at: std::time::Instant::now(),
    });
    let router = build_mcp_router(state, &config, password);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 出厂默认（回环、关闭远程访问）供大部分用例使用。
fn loopback_config() -> McpHttpConfig {
    McpHttpConfig::default()
}

/// MCP 的三条路由在守护进程那个端口上照常工作。
#[tokio::test]
async fn the_mcp_routes_answer_on_the_daemon_port() {
    let (addr, dir) = serve_mcp_router(loopback_config(), None).await;
    let client = reqwest::Client::new();

    for path in ["/mcp/healthz", "/mcp/tools"] {
        let response = client
            .get(format!("http://{addr}{path}"))
            .send()
            .await
            .expect("request completes");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "{path} should answer on the MCP daemon port"
        );
    }

    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .body(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
        )
        .send()
        .await
        .expect("request completes");
    assert!(
        response.status().is_success(),
        "initialize should be answered on the MCP daemon port"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["result"]["serverInfo"]["name"], "simple-post-mcp");

    let _ = std::fs::remove_dir_all(&dir);
}

/// bearer 口令守住 MCP 端点：没有令牌的请求被拒，带令牌的通过。
///
/// 守护进程是 MCP 唯一的对外端点，故它不能是一个没有闸的对外服务。
#[tokio::test]
async fn the_bearer_token_guards_the_mcp_endpoint() {
    let (addr, dir) = serve_mcp_router(loopback_config(), Some("hunter2")).await;
    let client = reqwest::Client::new();

    let denied = client
        .get(format!("http://{addr}/mcp/tools"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "没有令牌的请求必须被拒"
    );

    let allowed = client
        .get(format!("http://{addr}/mcp/tools"))
        .bearer_auth("hunter2")
        .send()
        .await
        .expect("request completes");
    assert_eq!(allowed.status(), reqwest::StatusCode::OK);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 健康探针不受口令影响：监管者与容器健康检查都不带令牌。
#[tokio::test]
async fn the_healthz_probe_stays_public() {
    let (addr, dir) = serve_mcp_router(loopback_config(), Some("hunter2")).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/mcp/healthz"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 配置里的挂载路径生效，`/mcp` 仍作为别名可用。
///
/// 别名不是装饰：AI 客户端按 `/mcp` 写死了地址，改名而不留别名就会把它们全部指空。
#[tokio::test]
async fn the_configured_mount_path_answers_and_mcp_stays_an_alias() {
    let mut config = loopback_config();
    config.path = "/mcp-endpoint".to_string();
    let (addr, dir) = serve_mcp_router(config, None).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{addr}/mcp-endpoint/healthz"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let response = client
        .get(format!("http://{addr}/mcp/healthz"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "/mcp 应当作为别名继续可用"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Host 白名单在那个端口上有约束力：不在名单里的 Host 被拒。
#[tokio::test]
async fn the_host_allowlist_applies_to_the_daemon_port() {
    let mut config = loopback_config();
    config.allow_remote = true;
    config.allowed_hosts = vec!["studio.local".to_string()];
    config.allowed_origins = vec!["*".to_string()];
    let (addr, dir) = serve_mcp_router(config, None).await;
    let client = reqwest::Client::new();

    let allowed = client
        .get(format!("http://{addr}/mcp/healthz"))
        .header("Host", "studio.local")
        .send()
        .await
        .expect("request completes");
    assert_eq!(allowed.status(), reqwest::StatusCode::OK);

    let denied = client
        .get(format!("http://{addr}/mcp/healthz"))
        .header("Host", "somewhere.else")
        .send()
        .await
        .expect("request completes");
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::FORBIDDEN,
        "不在白名单里的 Host 应当被拒"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
