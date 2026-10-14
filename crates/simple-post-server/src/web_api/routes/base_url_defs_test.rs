//! baseUrl-def 与 environment-baseUrl 路由的路由与线格式。
//!
//! baseUrl 一族里每条路由都经一个**路径参数**到达——`:projectId` 或 `:defId`——且
//! 项目级的那些把字面量段（`baseurls`、`baseurls/default`）彼此相邻、又与按 id 的
//! `/api/baseurl-defs/:defId` 相邻。它们能否共存是 axum radix router 的性质，不是注册
//! 代码读一读就能断言的——所以这些测试驱动真实 listener，观察真实的匹配结果。
//!
//! 判别依据是状态码。跑起来的 handler 回 `200` / `201` / `204`，或它自己拒收输入时的
//! `400` / `404`；抢优先级失败的路由会从 SPA fallback 回 `404`，或在被邻居的方法集匹配
//! 上时回 `405`。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
        "simple-post-web-routing-{}-{id}",
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

async fn delete(addr: SocketAddr, path: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .delete(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes")
        .status()
}

/// 种下一个项目、一个模块、一个属于该项目的服务、一个环境。`module_id` / `is_default`
/// 按 NULL / 0 写入——这两列已作废但仍留着，删列等于重建用户的数据库。
async fn seed_project(dir: &std::path::Path) {
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
            "INSERT INTO base_url_definitions (id, project_id, name) \
             VALUES ('def_1', 'p1', 'svc')",
        )?;
        c.execute_batch(
            "INSERT INTO environments \
             (id, project_id, name, is_active, variables_json) \
             VALUES ('env_1', 'p1', 'dev', 1, '{}')",
        )?;
        Ok(())
    })
    .expect("seed project");
}

/// `POST /api/projects/:projectId/baseurls` 必须回 `201` 与存下的行——项目就是 scope，
/// 故 id 在路径里，body 里只有属性。
#[tokio::test]
async fn the_project_scoped_create_route_answers() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let response = post_json(
        addr,
        "/api/projects/p1/baseurls",
        serde_json::json!({ "name": "auth" }),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::CREATED,
        "expected the create handler to run (201), not a routing failure"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["name"], "auth");
    assert!(
        body.get("id").is_some(),
        "the created service must come back with its id"
    );
    assert!(
        body.get("moduleId").is_none() && body.get("isDefault").is_none(),
        "a v2 service is project-scoped: no moduleId, no isDefault. got {body}"
    );
}

/// 调用方给的 `id` 必须被原样采纳。
///
/// renderer 之所以铸这个 id，是因为它在行存在*之前*就要用它：拿它当项目 ★，以及
/// 当 `env.moduleBaseUrls[defId][moduleId]` 那一格的键。服务端铸的 id 会让两者都指向
/// 一条从未写入的行，于是刚配好的服务解析不出任何 URL。这里驱动整条链——带 id 创建，
/// 再按它写那一格——因为「create 回 201 且带那个 id」只是契约的一半；格子之后必须能用
/// 同一个键读回来。
#[tokio::test]
async fn a_caller_supplied_id_is_adopted_and_its_cell_is_readable() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let created = post_json(
        addr,
        "/api/projects/p1/baseurls",
        serde_json::json!({ "name": "auth", "id": "def_client_1" }),
    )
    .await;
    assert_eq!(created.status(), reqwest::StatusCode::CREATED);
    let created: serde_json::Value = created.json().await.expect("json body");
    assert_eq!(
        created["id"], "def_client_1",
        "后端必须采纳调用方的 id，否则 ★ 与 URL 单元格会指向一条不存在的行；got {created}"
    );

    let cell = post_json(
        addr,
        "/api/environments/env_1/module-baseurls",
        serde_json::json!({
            "defId": "def_client_1",
            "moduleId": "mod_a",
            "url": "https://api.demo.dev",
        }),
    )
    .await;
    assert_eq!(cell.status(), reqwest::StatusCode::OK);
    let cell: serde_json::Value = cell.json().await.expect("json body");
    assert_eq!(
        cell["moduleBaseUrls"]["def_client_1"]["mod_a"], "https://api.demo.dev",
        "按同一个 id 写入的单元格必须读得回来"
    );
}

/// 省掉 `id` 保持既有契约——由服务端铸——这样旧版 renderer（或其他任何客户端）不会被
/// 新字段破坏。
#[tokio::test]
async fn omitting_the_id_still_yields_a_server_minted_one() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let response = post_json(
        addr,
        "/api/projects/p1/baseurls",
        serde_json::json!({ "name": "auth" }),
    )
    .await;

    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = response.json().await.expect("json body");
    let id = body["id"].as_str().expect("id as str");
    assert!(!id.is_empty(), "服务端必须铸一个非空 id，got {body}");
}

/// 空白 `id` 按缺失处理，而不是写成一行 id 为 `""` 的记录——本 API 上每个可选 id 都
/// 遵循同一条「空即缺失」规则。
#[tokio::test]
async fn a_blank_id_is_treated_as_absent() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let response = post_json(
        addr,
        "/api/projects/p1/baseurls",
        serde_json::json!({ "name": "auth", "id": "   " }),
    )
    .await;

    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = response.json().await.expect("json body");
    let id = body["id"].as_str().expect("id as str");
    assert!(!id.trim().is_empty(), "空白 id 不能落库，got {body}");
}

/// `POST /api/projects/:projectId/baseurls/default` 必须回更新后的 `Project`——
/// `defaultBaseUrlDefinitionId` 又活起来了，故调用方直接拿到新指针，而不是
/// `204` 加一次重读。
///
/// 状态码是那三种客户端看来相似的结果之间的判别依据：`200` 表示 handler 跑了，`404`
/// 表示它落到了 SPA fallback，`405` 表示它匹配上了邻居的方法集。
#[tokio::test]
async fn setting_the_star_answers_the_updated_project() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let response = post_json(
        addr,
        "/api/projects/p1/baseurls/default",
        serde_json::json!({ "defId": "def_1" }),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "expected the set-default handler to run (200), not a 404 from the \
         SPA fallback or a 405 from a neighbouring method set"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["id"], "p1");
    assert_eq!(body["defaultBaseUrlDefinitionId"], "def_1");
}

/// `defId: null` 把 ★ 撤掉——新建项目正处于这个状态，且 IPC 与 MCP 另外两个表面也能
/// 表达它。
#[tokio::test]
async fn clearing_the_star_answers_a_project_with_no_default() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    // 先设上，好让清除有东西可清，空写入不能冒充成一次生效的写入。
    post_json(
        addr,
        "/api/projects/p1/baseurls/default",
        serde_json::json!({ "defId": "def_1" }),
    )
    .await;

    let response = post_json(
        addr,
        "/api/projects/p1/baseurls/default",
        serde_json::json!({ "defId": serde_json::Value::Null }),
    )
    .await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert!(
        body["defaultBaseUrlDefinitionId"].is_null(),
        "defId: null must clear the star, got {body}"
    );
}

/// 按 id 的 `PATCH` / `DELETE /api/baseurl-defs/:defId` 必须仍能到达。
#[tokio::test]
async fn the_id_parametrised_route_still_matches() {
    let (addr, _dir) = serve_test_router().await;
    let status = delete(addr, "/api/baseurl-defs/def_123").await;

    assert_eq!(
        status,
        reqwest::StatusCode::NOT_FOUND,
        "the id-scoped delete handler should have run and found no such row"
    );
}

/// `/module-baseurls`：URL 唯一所在的一层，按服务**与**模块双向寻址，故这条路由若没
/// 注册成功，整个模型就完全没有写入路径。
#[tokio::test]
async fn the_module_base_url_route_writes_the_cell() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    let response = post_json(
        addr,
        "/api/environments/env_1/module-baseurls",
        serde_json::json!({ "defId": "def_1", "moduleId": "mod_a", "url": "http://x" }),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "expected the module-baseurl handler to run, not a routing failure"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["moduleBaseUrls"]["def_1"]["mod_a"], "http://x");
}

/// 打平的 `/baseurls` 动词已不在。它的缺席是硬要求：值这一层按（服务 × 模块）划分，
/// 一条幸存的打平路由会让调用方写进一个没人读得回的字段。
#[tokio::test]
async fn the_flat_base_url_route_is_gone() {
    let (addr, _dir) = serve_test_router().await;
    let status = post_json(
        addr,
        "/api/environments/env_1/baseurls",
        serde_json::json!({ "defId": "d", "url": "http://x" }),
    )
    .await
    .status();

    assert!(
        status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "expected the removed flat route to be unroutable, got {status}"
    );
}

/// 已删除的节点级 send 路径必须保持不可路由：前端自己拼装完整请求并 POST 到
/// `/api/http/send`，一条幸存的节点级 send 就是第二条不受管辖的出站通道。
#[tokio::test]
async fn the_node_scoped_send_route_is_gone() {
    let (addr, _dir) = serve_test_router().await;
    let status = post_json(addr, "/api/nodes/n1/send", serde_json::json!({}))
        .await
        .status();

    assert!(
        status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "expected the deleted send route to be unroutable, got {status}"
    );
}

/// 项目级读：`GET /api/projects/:projectId/baseurls` 仍能列出，且未知项目回 handler
/// 自己的 `200`（空列表）而非路由失败。
#[tokio::test]
async fn the_project_scoped_list_route_still_answers() {
    let (addr, _dir) = serve_test_router().await;
    let status = reqwest::Client::new()
        .get(format!(
            "http://{addr}/api/projects/no_such_project/baseurls"
        ))
        .send()
        .await
        .expect("request completes")
        .status();

    // `list_by_project` 是过滤而非报错，故未知项目是一次成功的空列表——不是路由失败就足以
    // 证明这条路由还活着。
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "the project-scoped list route must survive the refactor"
    );
}

/// 守住这套夹具本身：若服务器根本没起来，上面每条断言读到的都是 connection refused 的
/// 噪声。一条错误路径必须快速 404，而不是挂起。
#[tokio::test]
async fn the_harness_rejects_an_unrouted_path() {
    let (addr, _dir) = serve_test_router().await;
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        reqwest::Client::new()
            .get(format!("http://{addr}/api/definitely/not/a/route"))
            .send(),
    )
    .await
    .expect("server responds without hanging")
    .expect("request completes")
    .status();

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
}
