//! 专用 MCP 监听器的路由集合。
//!
//! 这个 router 只服务 MCP：管理平台的 REST 与 SPA 都不在里面。断言分两半写——
//! 「哪些路径可达」与「哪些路径 404」。只断言 `/mcp` 通是证明不了这件事的，
//! 因为一个把整棵主 router 挂上去的监听器也会让 `/mcp` 通。
//!
//! 它复用 daemon 那份 router（挂载路径 + Host/Origin 守卫），所以这里还要
//! 断言白名单确实生效：设置面板里那两张白名单表，对这个端口与对 daemon 是
//! 同一套规矩。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::McpHttpConfig;
use crate::web_api::{build_mcp_only_router, WebState};

/// 起一个只服务 MCP 的 router，绑在 loopback 的临时端口上。
///
/// 每个测试的隔离靠原子计数器加 pid，因为 cargo 并行跑测试，好几个会共用同一条临时路径。
async fn serve_mcp_only_router(config: McpHttpConfig) -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-mcp-only-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

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
    let router = build_mcp_only_router(state, &config);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 默认配置（回环、关闭远程访问）供大部分用例使用。
fn loopback_config() -> McpHttpConfig {
    McpHttpConfig::default()
}

/// MCP 那三条路由在这个端口上照常工作：两个探针 GET，`/mcp` 走 JSON-RPC。
#[tokio::test]
async fn the_mcp_routes_answer() {
    let (addr, dir) = serve_mcp_only_router(loopback_config()).await;
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
            "{path} should answer on the MCP-only listener"
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
        "initialize should be answered on the MCP-only listener"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["result"]["serverInfo"]["name"], "simple-post-mcp");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 管理平台在这个端口上不存在：SPA 壳、健康探针、REST 全部 404。
///
/// 这是「MCP 端口与管理平台端口不是同一个」的机器可查的证据——把 MCP 端口
/// 放给 AI 客户端，不会顺带把 `/api/*` 也放出去。
#[tokio::test]
async fn the_admin_surface_is_absent() {
    let (addr, dir) = serve_mcp_only_router(loopback_config()).await;
    let client = reqwest::Client::new();

    for path in ["/", "/healthz", "/api/healthz", "/api/projects"] {
        let response = client
            .get(format!("http://{addr}{path}"))
            .send()
            .await
            .expect("request completes");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND,
            "{path} must not exist on the MCP-only listener"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 设置里写的挂载路径在这个端口上生效，`/mcp` 仍作为别名可用。
///
/// 别名不是装饰：AI 客户端按 `/mcp` 写死了地址，改名而不留别名就会把它们
/// 全部指空。
#[tokio::test]
async fn the_configured_mount_path_answers_and_mcp_stays_an_alias() {
    let mut config = loopback_config();
    config.path = "/mcp-endpoint".to_string();
    let (addr, dir) = serve_mcp_only_router(config).await;
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

/// Host 白名单在这个端口上生效：不在名单里的 Host 被拒。
///
/// 这条断言钉的是「设置面板里那两张白名单表对这个端口同样有约束力」——
/// 专用端口不是绕过白名单的后门。
#[tokio::test]
async fn the_host_allowlist_applies_to_the_dedicated_port() {
    let mut config = loopback_config();
    config.allow_remote = true;
    config.allowed_hosts = vec!["studio.local".to_string()];
    config.allowed_origins = vec!["*".to_string()];
    let (addr, dir) = serve_mcp_only_router(config).await;
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
