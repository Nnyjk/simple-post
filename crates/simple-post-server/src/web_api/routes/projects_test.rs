//! `POST /api/projects` 的两种 body 形状。
//!
//! 两种形状都还在用：SPA 的 `invokeCmd(COMMANDS.projectsCreate, …)` 发的是
//! `{name, opts}`，外部 REST 客户端发的是打平的 `Project`。`Project` 没有 `opts` 字段，
//! 而 serde 会静默丢弃未知键——按 `Project` 解析会让 create 表单的 description 与 colour
//! 全被丢掉，只剩 name 返回。故这条路由两种形状都解析。
//!
//! 判别依据是响应体里那两个字段：**它们回来了**才说明形状被认了出来。状态码两种形状
//! 都是 `201`，分不出「形状认错了但照样建了」。

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

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-projects-{}-{id}",
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

async fn post_json(
    addr: SocketAddr,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/projects"))
        .json(&body)
        .send()
        .await
        .expect("request completes");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("json body");
    (status, payload)
}

/// SPA 用的 `{name, opts}` 形状：description 与 colour 在 `opts` 里。
#[tokio::test]
async fn the_opts_shape_keeps_the_description_and_the_colour() {
    let (addr, _dir) = serve_test_router().await;

    let (status, body) = post_json(
        addr,
        serde_json::json!({
            "name": "示例",
            "opts": { "description": "一段说明", "color": "#123456" },
        }),
    )
    .await;

    assert_eq!(status, 201, "建项目应当回 201");
    assert_eq!(body.get("name").and_then(|v| v.as_str()), Some("示例"));
    assert_eq!(
        body.get("description").and_then(|v| v.as_str()),
        Some("一段说明"),
        "`opts` 里的说明要落进这一行"
    );
    assert_eq!(
        body.get("color").and_then(|v| v.as_str()),
        Some("#123456"),
        "`opts` 里的颜色要落进这一行"
    );
}

/// 外部 REST 客户端用的打平 `Project` 形状：description 与 colour 在顶层。
#[tokio::test]
async fn the_flat_shape_keeps_the_description_and_the_colour() {
    let (addr, _dir) = serve_test_router().await;

    let (status, body) = post_json(
        addr,
        serde_json::json!({
            "name": "示例",
            "description": "一段说明",
            "color": "#123456",
        }),
    )
    .await;

    assert_eq!(status, 201, "建项目应当回 201");
    assert_eq!(
        body.get("description").and_then(|v| v.as_str()),
        Some("一段说明"),
        "打平形状里的说明要落进这一行"
    );
    assert_eq!(
        body.get("color").and_then(|v| v.as_str()),
        Some("#123456"),
        "打平形状里的颜色要落进这一行"
    );
}
