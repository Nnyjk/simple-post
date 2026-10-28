//! `PUT /api/request-settings` 的写入路径。
//!
//! 这一行（`preferences` 表里的 `request_settings`）带着 HTTPS 出站的三个证书路径字段，
//! 而 `GET /api/preferences` 把整行原样返回 —— 用户把私钥正文粘进路径输入框，那段文本
//! 就会原样落进这一行。
//!
//! 校验放在 core 的 `preferences::set` 里，故这一行无论从哪条写路径进来都拦得住。

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
        "simple-post-web-request-settings-{}-{id}",
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

/// 粘进来的证书正文必须被拒，且那一行不能留下半截。
#[tokio::test]
async fn a_pasted_certificate_body_is_rejected_and_nothing_is_written() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .put(format!("http://{addr}/api/request-settings"))
        .json(&serde_json::json!({
            "timeoutMs": 30000,
            "followRedirects": true,
            "maxResponseSizeKb": 5120,
            "tls": {
                "caCertificatePath":
                    "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----"
            }
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "粘进来的证书正文必须被拒"
    );

    let response = client
        .get(format!("http://{addr}/api/request-settings"))
        .send()
        .await
        .expect("request completes");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert!(
        body["tls"]["caCertificatePath"].is_null(),
        "被拒的写入不许在库里留下半截，got: {body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 正常路径照旧写得进去 —— 上面那条校验拦的是证书正文，不是路径。
#[tokio::test]
async fn an_ordinary_certificate_path_is_accepted_and_read_back() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .put(format!("http://{addr}/api/request-settings"))
        .json(&serde_json::json!({
            "timeoutMs": 30000,
            "followRedirects": true,
            "maxResponseSizeKb": 5120,
            "tls": {
                "skipVerify": true,
                "caCertificatePath": "C:\\certs\\company-ca.pem"
            }
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let response = client
        .get(format!("http://{addr}/api/request-settings"))
        .send()
        .await
        .expect("request completes");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(
        body["tls"]["caCertificatePath"],
        serde_json::json!("C:\\certs\\company-ca.pem")
    );
    assert_eq!(body["tls"]["skipVerify"], serde_json::json!(true));

    let _ = std::fs::remove_dir_all(&dir);
}
