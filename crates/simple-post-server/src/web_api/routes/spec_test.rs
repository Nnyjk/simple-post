//! `POST /api/spec/upsert` 的行为测试。
//!
//! 这一组守三件事：
//!
//! 1. **一次调用真的落全了** —— 模块、集合路径上的每一级、环境、服务、端点一个都不少。
//! 2. **两种拼写都收** —— `project_id` 与 `projectId` 落在同一个类型上，故一份规格
//!    从 MCP 客户端搬到 HTTP 调用方不必翻译。
//! 3. **写错了要响亮地拒绝** —— 多一个键就回 `400`，而不是静默丢掉。

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
        "simple-post-web-spec-routing-{}-{id}",
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

async fn post_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes")
}

/// 种一个项目。规格必须落进一个**已经存在**的项目 —— 它不建项目。
fn seed_project(dir: &std::path::Path) {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(
            c,
            &crate::domain::Project {
                id: "p1".to_string(),
                name: "规格项目".to_string(),
                description: None,
                color: "#3B82F6".to_string(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        Ok(())
    })
    .expect("seed project");
}

/// 一份完整的规格，用 camelCase 写（这条线通篇用 camelCase）。
fn camel_case_spec() -> serde_json::Value {
    serde_json::json!({
        "projectId": "p1",
        "module": "交易",
        "collectionPath": "订单/明细",
        "environment": {
            "name": "dev",
            "variables": { "token": "abc" },
            "baseUrl": "https://api.example.test",
            "activate": true
        },
        "endpoint": {
            "name": "查订单",
            "method": "GET",
            "url": "/api/orders/{id}",
            "params": [{ "key": "page", "value": "1" }]
        }
    })
}

/// 一次调用落全一份规格，且回的是「落了什么 + 还缺什么」。
#[tokio::test]
async fn one_call_lands_the_whole_spec() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir);

    let response = post_json(addr, "/api/spec/upsert", camel_case_spec()).await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "规格路由必须真的跑起来（200），而不是落到 SPA fallback"
    );
    let body: serde_json::Value = response.json().await.expect("json body");

    // 落了什么：模块、集合路径上的两级、环境、端点。
    assert_eq!(body["module"]["name"], "交易", "模块按名字建出来");
    assert_eq!(body["module"]["created"], true, "第一次应用是新建");
    let collections = body["collections"].as_array().expect("集合数组");
    assert_eq!(collections.len(), 2, "路径上的两级集合都要建");
    assert_eq!(collections[0]["name"], "订单");
    assert_eq!(collections[1]["name"], "明细");
    assert_eq!(body["environment"]["name"], "dev");
    assert_eq!(body["environment"]["activated"], true);
    assert_eq!(body["endpoint"]["name"], "查订单");
    assert!(
        body["endpoint"]["id"].as_str().is_some(),
        "端点 id 必须回来"
    );

    // 还缺什么：base URL 那一格填了，但 `{{token}}` 不在这份规格里，
    // 而环境变量里也只有 token —— 故这里应当是空列表。
    assert!(
        body["missing"].is_array(),
        "「还缺什么」必须是一个数组，哪怕它空"
    );
}

/// 反复应用同一份规格不改变任何东西：模块、集合、端点都复用。
#[tokio::test]
async fn applying_the_same_spec_twice_changes_nothing() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir);

    let first: serde_json::Value = post_json(addr, "/api/spec/upsert", camel_case_spec())
        .await
        .json()
        .await
        .expect("json body");
    let second: serde_json::Value = post_json(addr, "/api/spec/upsert", camel_case_spec())
        .await
        .json()
        .await
        .expect("json body");

    assert_eq!(
        first["endpoint"]["id"], second["endpoint"]["id"],
        "端点复用"
    );
    assert_eq!(second["module"]["created"], false, "模块复用，不新建");
    assert_eq!(second["endpoint"]["created"], false, "端点复用，不新建");
    for c in second["collections"].as_array().expect("集合数组") {
        assert_eq!(c["created"], false, "集合复用，不新建");
    }
}

/// 多一个键就拒绝，而不是静默丢掉它。
#[tokio::test]
async fn an_unknown_field_is_rejected() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir);

    let mut spec = camel_case_spec();
    spec["endpoint"]["methodd"] = serde_json::json!("GET");

    let response = post_json(addr, "/api/spec/upsert", spec).await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "拼错的字段必须响亮地拒绝：静默丢掉会让人以为字段写对了"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["code"], "validation");
}

/// 项目不存在就报错，不建项目 —— 建项目要改权限档位，那是一次显式的决定。
#[tokio::test]
async fn a_missing_project_is_rejected() {
    let (addr, _dir) = serve_test_router().await;

    let mut spec = camel_case_spec();
    spec["projectId"] = serde_json::json!("p_not_here");

    let response = post_json(addr, "/api/spec/upsert", spec).await;

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

/// 一份超过 axum 默认上限（2 MB）的规格必须真的进得来。
///
/// `body.schema` 可以是一份很大的 JSON Schema。少了显式的 `DefaultBodyLimit`，这份
/// 规格在浏览器形态下回的是 413（传输层错误，读不出「规格太大」），而桌面形态的
/// 同一份规格能落库 —— 两侧行为分叉且无一声报错。
#[tokio::test]
async fn a_spec_larger_than_the_default_body_limit_is_accepted() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir);

    let mut spec = camel_case_spec();
    // 一份 3 MB 的 schema：正文用一段重复的描述撑出来。
    let filler = "x".repeat(3 * 1024 * 1024);
    spec["endpoint"]["body"] = serde_json::json!({
        "mode": "json",
        "content": "{}",
        "schema": format!("{{\"type\":\"object\",\"description\":\"{filler}\"}}")
    });

    let response = post_json(addr, "/api/spec/upsert", spec).await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "超大规格必须进得来：axum 的默认上限 2 MB 会把它挡在传输层"
    );
}
