//! collection 脚本路由的真实请求。
//!
//! 跑真实 listener：路由能否与 `/api/nodes/{id}` 共存、`GET` 无行时回什么、`PATCH`
//! 的三态（缺席 / 显式 null / 设值）在线上是否成立——这些都不是读注册代码能断言的。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-node-scripts-{}-{id}",
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

/// 种下一个项目、一个模块、一个 collection。
fn seed(dir: &std::path::Path) {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p1', 'demo', '#000', 1, 1)",
        )?;
        c.execute_batch(
            "INSERT INTO nodes (id, root_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('mod_a', 'p1', 'module', 'A', 0, 1, 1)",
        )?;
        c.execute_batch(
            "INSERT INTO nodes (id, root_id, parent_id, type, name, sort_order, \
                                created_at, updated_at) \
             VALUES ('col_a', 'p1', 'mod_a', 'collection', 'C', 0, 1, 1)",
        )?;
        Ok(())
    })
    .expect("seed tree");
}

async fn get_json(addr: SocketAddr, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes")
}

async fn patch_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .patch(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes")
}

#[tokio::test]
async fn a_collection_without_scripts_answers_an_empty_pair() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let response = get_json(addr, "/api/nodes/col_a/scripts").await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["nodeId"], "col_a");
    assert!(body["preRequestScript"].is_null());
    assert!(body["testScript"].is_null());
}

#[tokio::test]
async fn both_scripts_can_be_written_and_read_back() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let response = patch_json(
        addr,
        "/api/nodes/col_a/scripts",
        serde_json::json!({
            "preRequestScript": "pm.request.url = 'x';",
            "testScript": "pm.test('t', function () {});"
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["preRequestScript"], "pm.request.url = 'x';");
    assert_eq!(body["testScript"], "pm.test('t', function () {});");

    let read = get_json(addr, "/api/nodes/col_a/scripts").await;
    let body: serde_json::Value = read.json().await.expect("json body");
    assert_eq!(body["preRequestScript"], "pm.request.url = 'x';");
}

#[tokio::test]
async fn an_explicit_null_clears_one_script_and_leaves_the_other() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    patch_json(
        addr,
        "/api/nodes/col_a/scripts",
        serde_json::json!({ "preRequestScript": "a", "testScript": "b" }),
    )
    .await;

    let response = patch_json(
        addr,
        "/api/nodes/col_a/scripts",
        serde_json::json!({ "preRequestScript": null }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert!(body["preRequestScript"].is_null());
    assert_eq!(body["testScript"], "b");
}

#[tokio::test]
async fn a_module_cannot_carry_scripts() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let response = patch_json(
        addr,
        "/api/nodes/mod_a/scripts",
        serde_json::json!({ "preRequestScript": "a" }),
    )
    .await;
    assert_eq!(response.status(), 400, "模块不该能带脚本");
}
