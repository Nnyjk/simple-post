//! 动态变量路由的真实请求。
//!
//! 跑真实 listener：字面量段 `/resolve` 能否与 `/{id}` 共存、列表只读缓存不取数、
//! 覆盖值的写入与清除、`PATCH` 的三态在线上是否成立——这些都不是读注册代码能断言的。

use std::net::SocketAddr;
use std::path::PathBuf;

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
        serde_json::json!({ "projectId": "p1", "name": "sns", "extractPath": "$.list", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    assert_eq!(created.status(), 201);
    let created_body: serde_json::Value = created.json().await.expect("json body");
    assert_eq!(created_body["name"], "sns");
    assert_eq!(created_body["projectId"], "p1");
    let id = created_body["id"].as_str().expect("id").to_string();

    let listed = get_json(addr, "/api/dynamic-variables?projectId=p1").await;
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
async fn the_list_filters_by_project() {
    // 定义挂在项目里，故列表按项目过滤：列 p1 拿不到 p2 的定义。
    // 不带过滤参数时列的是全部项目——那是「调用接口跨项目取值」那条路的读侧。
    let (addr, dir) = serve_test_router().await;
    seed(&dir);
    let pool = crate::db::init(&dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p2', 'other', '#000', 2, 2)",
        )?;
        Ok(())
    })
    .expect("seed second project");

    for (project_id, name) in [("p1", "sns"), ("p2", "sns")] {
        let created = post_json(
            addr,
            "/api/dynamic-variables",
            serde_json::json!({ "projectId": project_id, "name": name, "sourceInlineJson": INLINE_SOURCE }),
        )
        .await;
        assert_eq!(created.status(), 201, "同名变量在两个项目里都该建得出来");
    }

    let listed = get_json(addr, "/api/dynamic-variables?projectId=p1").await;
    let items: serde_json::Value = listed.json().await.expect("json body");
    let items = items.as_array().expect("数组");
    assert_eq!(items.len(), 1, "只该列 p1 的那一条");
    assert_eq!(items[0]["projectId"], "p1");

    let all = get_json(addr, "/api/dynamic-variables").await;
    let all: serde_json::Value = all.json().await.expect("json body");
    assert_eq!(
        all.as_array().expect("数组").len(),
        2,
        "不带过滤参数时列的是全部项目"
    );
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
        serde_json::json!({ "projectId": "p1", "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
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
        serde_json::json!({ "projectId": "p1", "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
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
        serde_json::json!({ "projectId": "p1", "name": "sns-list" }),
    )
    .await;
    assert_eq!(created.status(), 400);
    let body: serde_json::Value = created.json().await.expect("json body");
    assert_eq!(body["code"], "validation");
}

/// 回固定 JSON 的假上游。预览是真取一次数，故它需要一个真响应的来源。
async fn upstream() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let port = listener.local_addr().expect("upstream addr").port();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8 * 1024];
            let Ok(n) = sock.read(&mut buf).await else {
                continue;
            };
            let _ = n;
            let body = r#"{"points":[{"code":"A001"},{"code":"B002"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    format!("http://127.0.0.1:{port}/points")
}

#[tokio::test]
async fn the_literal_preview_route_is_not_swallowed_by_the_id_route() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);
    let points_url = upstream().await;

    // `/api/dynamic-variables/preview` 必须落到字面量那条路由上。若被 `/{id}` 吃掉，
    // POST 会撞上「改一条定义」那条（只有 PATCH/DELETE）并回 405。
    let preview = post_json(
        addr,
        "/api/dynamic-variables/preview",
        serde_json::json!({
            "name": "sns",
            "sourceInlineJson": format!(r#"{{"method":"GET","url":"{points_url}"}}"#),
            "extractPath": "$.points[0].code",
        }),
    )
    .await;
    assert_eq!(preview.status(), 200);
    let body: serde_json::Value = preview.json().await.expect("json body");
    // 取数真发出去了：正文与元数据都在。
    assert_eq!(body["status"], 200);
    assert_eq!(body["contentType"], "application/json");
    assert_eq!(body["binary"], false);
    assert_eq!(body["truncated"], false);
    // 抽取规则生效，最终值就是抽取结果（没有处理脚本）。
    assert_eq!(body["extractPath"], "$.points[0].code");
    assert_eq!(body["extracted"], "A001");
    assert_eq!(body["value"], "A001");
    // 抽取成功时**正文也要给**，否则树没东西可点。
    assert_eq!(body["bodyJson"]["points"][1]["code"], "B002");
    // 内联请求没有环境，故这一格缺席（序列化时被跳过），界面据此说「没有环境」。
    assert!(body.get("environmentId").is_none());
}

#[tokio::test]
async fn a_preview_that_cannot_extract_still_returns_the_body() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);
    let points_url = upstream().await;

    // 路径没命中：预览不整体失败——正文与树照给，错误单独记一条，用户才能照着树重新点。
    let preview = post_json(
        addr,
        "/api/dynamic-variables/preview",
        serde_json::json!({
            "name": "sns",
            "sourceInlineJson": format!(r#"{{"method":"GET","url":"{points_url}"}}"#),
            "extractPath": "$.nope",
        }),
    )
    .await;
    assert_eq!(preview.status(), 200);
    let body: serde_json::Value = preview.json().await.expect("json body");
    assert_eq!(body["extractError"]["kind"], "no_match");
    assert!(body.get("extracted").is_none());
    assert_eq!(body["bodyJson"]["points"][0]["code"], "A001");
}

#[tokio::test]
async fn an_empty_refresh_body_is_treated_as_no_body() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir);

    let created = post_json(
        addr,
        "/api/dynamic-variables",
        serde_json::json!({ "projectId": "p1", "name": "sns", "sourceInlineJson": INLINE_SOURCE }),
    )
    .await;
    let created_body: serde_json::Value = created.json().await.expect("json body");
    let id = created_body["id"].as_str().expect("id").to_string();
    let url = format!("http://{addr}/api/dynamic-variables/{id}/refresh");

    // 带 `Content-Type: application/json` 的空 body：改动前 extractor 先回 400，刷新根本没执行。
    let with_header = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body("")
        .send()
        .await
        .expect("request completes");
    assert_eq!(with_header.status(), 200, "空 body 当没有 body");
    let body: serde_json::Value = with_header.json().await.expect("json body");
    // 取数打不到 127.0.0.1:9：刷新照跑，失败记在 error 上，不让整条请求失败。
    assert_eq!(body["source"], "missing");
    assert!(body["error"].is_object(), "取数失败要有 error：{}", body);

    // 不带那个 header 的空 body 一直就是没有 body。
    let without_header = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .expect("request completes");
    assert_eq!(without_header.status(), 200);

    // 非空 body 才解析：坏 JSON 仍是校验错误。
    let broken = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body("{")
        .send()
        .await
        .expect("request completes");
    assert_eq!(broken.status(), 400);
    let broken_body: serde_json::Value = broken.json().await.expect("json body");
    assert_eq!(broken_body["code"], "validation");
}
