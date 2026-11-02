//! `POST /api/nodes` 的线格式：body 里没有 `id` 时由服务端补上。
//!
//! `NodeCreateNodeBody.id` 是必填字段，而解析它之前 `ensure_server_id` 已经把缺失的 id
//! 补上了——这一步少了，body 里不带 id 的请求会回 400。SPA 建模块时发的那份 body 就
//! 不带 id。
//!
//! 判别依据是状态码与响应体：补上的 id 出现在响应里，才是「服务端补的」而不是
//! 「客户端自己带的」。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 起一个真实的 router：一次性的 SQLite 文件 + 空 SPA 目录，绑在 loopback 的临时端口上。
///
/// 计数器是每模块各自的：跨两个 `#[path]` 测试模块共享一套夹具需要一个谁都不拥有的
/// `pub(crate)` 落点，故这里各写一份。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir =
        std::env::temp_dir().join(format!("simple-post-web-nodes-{}-{id}", std::process::id()));
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

/// 一个项目：返回它的 id。顶层模块的父是项目，故模块要指着自己的项目。
async fn seed_project(dir: &std::path::Path) -> String {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p1', 'demo', '#000', 1, 1)",
        )?;
        Ok(())
    })
    .expect("seed project");
    "p1".to_string()
}

#[tokio::test]
async fn a_body_without_an_id_gets_one_from_the_server() {
    let (addr, dir) = serve_test_router().await;
    let project_id = seed_project(&dir).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/nodes"))
        .json(&serde_json::json!({
            "kind": "module",
            "name": "新建模块",
            "rootId": project_id,
        }))
        .send()
        .await
        .expect("request completes");

    assert_eq!(
        response.status(),
        201,
        "body 里没有 id 时服务端要补一个，不能回 400"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    assert!(!id.is_empty(), "服务端补的 id 要出现在响应里");
    assert_eq!(
        body.get("name").and_then(|v| v.as_str()),
        Some("新建模块"),
        "补 id 不能把 body 里别的字段弄丢"
    );
}
