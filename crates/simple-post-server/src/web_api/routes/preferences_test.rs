//! `PUT /api/preferences` 的写入路径。
//!
//! 这一条路由经手所有偏好设置的写入，而偏好行存的是 **JSON 编码后的值**。写入时
//! 多编码一次，存下去的就是字符串的字符串：`true` 变成 `"true"`，读侧的
//! `get::<Option<bool>>` 解析失败，`serve()` 里的 `unwrap_or(false)` 再把失败吞成
//! false——于是「随应用启动 MCP」这类开关写进去也等于没写。
//!
//! 所以这里的断言不只看 HTTP 状态码，还要看**读回来是什么类型**。

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
        "simple-post-web-preferences-{}-{id}",
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

/// 写进去的布尔值，读回来仍然是布尔值。
///
/// 这条断言盯的是**类型**：写成字符串时读回侧会解析失败，而失败的痕迹在 HTTP 响应
/// 上完全看不出来（写入照样回 204，读取照样回 200）。
#[tokio::test]
async fn a_boolean_preference_round_trips_as_a_boolean() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .put(format!("http://{addr}/api/preferences"))
        .json(&serde_json::json!({ "key": "mcp.auto_start", "value": true }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = client
        .get(format!("http://{addr}/api/preferences"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");

    assert_eq!(
        body["mcp.auto_start"],
        serde_json::json!(true),
        "mcp.auto_start 读回来必须是布尔值，而不是字符串"
    );
    assert!(
        body["mcp.auto_start"].is_boolean(),
        "多编码一次时它会变成字符串 \"true\"，读侧解析失败后被 unwrap_or(false) 吞掉"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 对象与数字同样按原类型往返。
#[tokio::test]
async fn an_object_preference_round_trips_as_an_object() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .put(format!("http://{addr}/api/preferences"))
        .json(&serde_json::json!({
            "key": "mcp.http_config",
            "value": { "host": "0.0.0.0", "port": 54174, "path": "/mcp" },
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = client
        .get(format!("http://{addr}/api/preferences"))
        .send()
        .await
        .expect("request completes");
    let body: serde_json::Value = response.json().await.expect("json body");

    assert_eq!(body["mcp.http_config"]["port"], serde_json::json!(54174));
    assert!(body["mcp.http_config"].is_object());

    let _ = std::fs::remove_dir_all(&dir);
}

/// `request_settings` 这一行带着证书路径，而 `GET /api/preferences` 把整行原样返回。
///
/// 这一行有三条写路径，`PUT /api/preferences` 是其中最宽的一条（它能写任何键）。校验放在
/// core 的 `preferences::set` 里，故这条路径同样拦得住 —— 拦不住的话，用户把私钥正文
/// 粘进路径输入框，那段文本就会原样落进这一行，而这一行能被任何人读走。
#[tokio::test]
async fn writing_request_settings_through_the_raw_route_rejects_a_pasted_certificate_body() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    let response = client
        .put(format!("http://{addr}/api/preferences"))
        .json(&serde_json::json!({
            "key": "request_settings",
            "value": {
                "timeoutMs": 30000,
                "followRedirects": true,
                "maxResponseSizeKb": 5120,
                "tls": {
                    "caCertificatePath":
                        "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----"
                }
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

    // 写不进去的那一行也不能留下半截。
    let response = client
        .get(format!("http://{addr}/api/preferences"))
        .send()
        .await
        .expect("request completes");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert!(
        body.get("request_settings").is_none(),
        "被拒的写入不许在库里留下那一行，got: {body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 形状不对的请求体必须被拒，且那一行保持原样。
///
/// 这一行只存出站请求设置，故通用写入口写来的值若读不出 `RequestSettings`，它就是一个
/// 形状不对的请求体 —— 不是「这一行不归我管」。放过它就留下一条绕路：`timeoutMs` 的类型
/// 写错让整体反序列化失败，证书路径的校验因此根本没跑，而同一个请求体里的
/// `tls.clientKeyPath` 正粘着私钥正文。
#[tokio::test]
async fn writing_request_settings_through_the_raw_route_rejects_a_broken_shape() {
    let (addr, dir) = serve_test_router().await;
    let client = reqwest::Client::new();

    // 先落一行正常的设置，好断言被拒的写入没有把它改掉。
    let response = client
        .put(format!("http://{addr}/api/request-settings"))
        .json(&serde_json::json!({
            "timeoutMs": 12345,
            "followRedirects": false,
            "maxResponseSizeKb": 2048,
            "tls": { "skipVerify": false }
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let response = client
        .put(format!("http://{addr}/api/preferences"))
        .json(&serde_json::json!({
            "key": "request_settings",
            "value": {
                "timeoutMs": "invalid",
                "tls": {
                    "clientKeyPath":
                        "-----BEGIN PRIVATE KEY-----\nexample\n-----END PRIVATE KEY-----"
                }
            }
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "形状不对的请求体必须被拒"
    );

    // 那一行保持原样，且整行文本里没有私钥正文。
    let response = client
        .get(format!("http://{addr}/api/preferences"))
        .send()
        .await
        .expect("request completes");
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(
        body["request_settings"]["timeoutMs"],
        serde_json::json!(12345),
        "被拒的写入不许改动那一行，got: {body}"
    );
    assert_eq!(
        body["request_settings"]["maxResponseSizeKb"],
        serde_json::json!(2048),
        "被拒的写入不许改动那一行，got: {body}"
    );
    assert!(
        !body.to_string().contains("BEGIN PRIVATE KEY"),
        "那一行里不许出现私钥正文，got: {body}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
