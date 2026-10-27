//! 动态变量路由的真实请求。
//!
//! 跑真实 listener：字面量段 `/resolve` 能否与 `/{id}` 共存、列表只读缓存不取数、
//! 覆盖值的写入与清除、`PATCH` 的三态在线上是否成立——这些都不是读注册代码能断言的。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-dynamic-vars-{}-{id}",
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

/// 种下一个项目。
fn seed(dir: &std::path::Path) {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p1', 'demo', '#000', 1, 1)",
        )?;
        Ok(())
    })
    .expect("seed project");
}

async fn get_json(addr: SocketAddr, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes")
}

async fn post_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .json(&body)
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
async fn creating_and_listing_a_definition_round_trips() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "name": "sns", "extractPath": "$.list", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    assert_eq!(created.status(), 201);
    let created_body: serde_json::Value = created.json().await.expect("json body");
    assert_eq!(created_body["name"], "sns");
    let id = created_body["id"].as_str().expect("id").to_string();

    let listed = get_json(addr, "/api/dynamic-variables").await;
    assert_eq!(listed.status(), 200);
    let listed_body: serde_json::Value = listed.json().await.expect("json body");
    let items = listed_body.as_array().expect("数组");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "sns");
    // 没有缓存值时来源是「没有值」——列表只读缓存，不取数。
    assert_eq!(items[0]["source"], "missing");
    assert!(items[0]["value"].is_null());

    let _ = id;
}

#[tokio::test]
async fn the_literal_resolve_route_is_not_swallowed_by_the_id_route() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    // `/api/dynamic-variables/resolve` 必须落到字面量那条路由上。
    // 若被 `/{id}` 吃掉，它会按「改一条定义」处理并回 400/422。
    let resolved = post_json(
        addr,
        "/api/dynamic-variables/resolve",
        serde_json::json!({ "names": ["sns"] }),
    )
    .await;
    assert_eq!(resolved.status(), 200);
    let body: serde_json::Value = resolved.json().await.expect("json body");
    assert!(body["values"].is_object());
    assert!(body["statuses"].is_array());
}

#[tokio::test]
async fn an_override_is_written_and_cleared() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    let created_body: serde_json::Value = created.json().await.expect("json body");
    let id = created_body["id"].as_str().expect("id").to_string();

    let set = post_json(
        addr,
        &format!("/api/dynamic-variables/{id}/override"),
        serde_json::json!({ "value": "[{\"code\":\"A001\"}]" }),
    )
    .await;
    assert_eq!(set.status(), 200);
    let set_body: serde_json::Value = set.json().await.expect("json body");
    assert_eq!(set_body["source"], "override");
    assert!(set_body["isOverride"].as_bool().unwrap_or(false));
    assert!(set_body["value"]
        .as_str()
        .unwrap_or_default()
        .contains("A001"));

    let cleared = post_json(
        addr,
        &format!("/api/dynamic-variables/{id}/override"),
        serde_json::json!({ "value": null }),
    )
    .await;
    assert_eq!(cleared.status(), 200);
    let cleared_body: serde_json::Value = cleared.json().await.expect("json body");
    assert_eq!(cleared_body["source"], "missing");
}

#[tokio::test]
async fn a_patch_changes_the_definition_and_a_delete_removes_it() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    let created_body: serde_json::Value = created.json().await.expect("json body");
    let id = created_body["id"].as_str().expect("id").to_string();

    let patched = patch_json(
        addr,
        &format!("/api/dynamic-variables/{id}"),
        serde_json::json!({ "name": "factors", "ttlSeconds": 30 }),
    )
    .await;
    assert_eq!(patched.status(), 200);
    let patched_body: serde_json::Value = patched.json().await.expect("json body");
    assert_eq!(patched_body["name"], "factors");
    assert_eq!(patched_body["ttlSeconds"], 30);

    let deleted = reqwest::Client::new()
        .delete(format!("http://{addr}/api/dynamic-variables/{id}"))
        .send()
        .await
        .expect("request completes");
    assert_eq!(deleted.status(), 204);

    let listed = get_json(addr, "/api/dynamic-variables").await;
    let listed_body: serde_json::Value = listed.json().await.expect("json body");
    assert!(listed_body.as_array().expect("数组").is_empty());
}

#[tokio::test]
async fn an_illegal_name_is_a_validation_error() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "name": "sns-list" }),
    )
    .await;
    assert_eq!(created.status(), 400);
    let body: serde_json::Value = created.json().await.expect("json body");
    assert_eq!(body["code"], "validation");
}

#[tokio::test]
async fn skipping_the_references_leaves_them_out() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);
    seed_an_endpoint_that_uses_sns(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    assert_eq!(created.status(), 201);

    // 默认带上引用关系。
    let listed = get_json(addr, "/api/dynamic-variables").await;
    assert_eq!(listed.status(), 200);
    let body: serde_json::Value = listed.json().await.expect("json body");
    let items = body.as_array().expect("数组");
    assert_eq!(items[0]["referencedBy"].as_array().expect("引用").len(), 1);

    // 传 `includeReferences=false` 就不带它：算引用关系要扫一遍全部端点。
    let skipped = get_json(addr, "/api/dynamic-variables?includeReferences=false").await;
    assert_eq!(skipped.status(), 200);
    let body: serde_json::Value = skipped.json().await.expect("json body");
    let items = body.as_array().expect("数组");
    assert!(items[0]["referencedBy"]
        .as_array()
        .expect("引用")
        .is_empty());
}

/// 种下一个模块、一个 collection 与一个请求体里写了 `{{sns}}` 的接口。
fn seed_an_endpoint_that_uses_sns(dir: &std::path::Path) {
    use crate::domain::{
        AuthConfig, BodyMode, EndpointDetail, HttpMethod, RequestBody, ResourceNode,
        ResourceNodeType,
    };

    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        let node =
            |id: &str, kind: ResourceNodeType, parent: Option<&str>, sort: i64| ResourceNode {
                id: id.into(),
                root_id: "p1".into(),
                parent_id: parent.map(str::to_string),
                kind,
                name: id.into(),
                description: None,
                sort_order: sort,
                created_at: 1,
                updated_at: 1,
            };
        crate::db::nodes::create(c, &node("m1", ResourceNodeType::Module, None, 0), None)?;
        crate::db::nodes::create(
            c,
            &node("c1", ResourceNodeType::Collection, Some("m1"), 0),
            None,
        )?;
        let detail = EndpointDetail {
            node_id: "e1".into(),
            method: HttpMethod::Post,
            url: "/consume".into(),
            environment_id: None,
            base_url_definition_id: None,
            base_url_override: None,
            params: vec![],
            path_params: vec![],
            headers: vec![],
            body: RequestBody {
                mode: BodyMode::Json,
                content: "{\"sns\": {{sns}}}".into(),
                schema: None,
            },
            auth: AuthConfig::default(),
            docs: None,
            notes: None,
            tags: vec![],
            pre_request_script: None,
            test_script: None,
        };
        crate::db::nodes::create(
            c,
            &node("e1", ResourceNodeType::Endpoint, Some("c1"), 0),
            Some(&detail),
        )?;
        Ok(())
    })
    .expect("seed tree");
}
