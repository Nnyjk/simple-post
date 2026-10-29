//! 响应历史路由的路由与线格式。
//!
//! 驱动真实 listener：`/api/response-history` 与
//! `/api/response-history/:endpointId` 能否在 axum 的 radix tree 里共存，是 router 的
//! 性质，不是注册代码读一读就能断言的。失败形态之所以难缠，是因为它是静默的——抢
//! 优先级失败的路由会落到 SPA fallback，回 `200` 加一个 HTML body，在 curl 看来与一个
//! 「恰好忽略自己路径」的正常端点毫无区别。
//!
//! 故全篇的判别依据是 *body*：这些测试把 JSON 读回来，而不只是看状态码。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use crate::web_api::{build_router, WebState};

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
///
/// 计数器是每模块各自的：跨两个 `#[path]` 测试模块共享一套夹具需要一个谁都不拥有的
/// `pub(crate)` 落点，故这里各写一份。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-response-history-{}-{id}",
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

/// 种下一个项目，含一个模块与其下一个 endpoint。
///
/// 这个 endpoint 正是 `response_history.endpoint_id` 引用的对象，而它冗余的
/// `nodes.root_id` 列正是项目级列表 join 所经的路径——种一个没有 root 的 endpoint
/// 会让列表理所当然地空掉、append 撞上外键，于是下面每个测试都长得像路由 bug。
async fn seed_tree(dir: &std::path::Path) {
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
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e1', 'p1', 'mod_a', 'endpoint', 'GET /orders', 0, 1, 1)",
        )?;
        // 一个兄弟 endpoint，好让作用域测试有两个可分辨的对象。
        // `response_history.endpoint_id` 是指向 `nodes` 的外键，故点名 `e2` 的
        // append 必须找得到真实行。
        c.execute_batch(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e2', 'p1', 'mod_a', 'endpoint', 'GET /users', 1, 1, 1)",
        )?;
        Ok(())
    })
    .expect("seed tree");
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn get_json(addr: SocketAddr, path: &str) -> (reqwest::StatusCode, serde_json::Value) {
    let res = client()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn post_json(
    addr: SocketAddr,
    path: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let res = client()
        .post(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn delete_json(addr: SocketAddr, path: &str) -> (reqwest::StatusCode, serde_json::Value) {
    let res = client()
        .delete(format!("http://{addr}{path}"))
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

/// renderer 把 `ApiResponse` 的 header 对经 `tuplesToRecord` 折叠后发的 body。`bodyJson`
/// 整个省略——domain 类型带 `skip_serializing_if`，故调用方从未解析过的响应就是
/// 没有这个键的形状，路由必须能接受。
fn append_body() -> serde_json::Value {
    serde_json::json!({
        "endpointId": "e1",
        "response": {
            "status": 200,
            "statusText": "OK",
            "durationMs": 42,
            "sizeBytes": 2,
            "headers": { "content-type": "application/json" },
            "body": "{}",
            "contentType": "application/json",
            "method": "get",
            "url": "http://x/orders"
        }
    })
}

/// 先 `POST` 再 `GET`——历史条依赖的往返，且任一条路由从未注册都会让它响亮地失败
/// （缺路由时回的是 SPA shell 的 `200` 加 HTML body，这里就会拿 HTML 去比一个列表）。
#[tokio::test]
async fn append_then_list_answers_both_routes() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    let (status, entry) = post_json(addr, "/api/response-history", append_body()).await;
    assert_eq!(status, 201, "append route must answer, got {entry}");
    assert_eq!(entry["endpointId"], "e1");
    assert_eq!(entry["response"]["status"], 200);
    // 这两个都由服务端铸造；调用方的 body 一个都没带。
    assert!(entry["id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(entry["createdAt"].as_i64().is_some_and(|n| n > 0));
    // 省略了它们时不应要求 renderer 发送的默认值。
    assert_eq!(entry["bodyTruncated"], false);
    assert!(entry["response"]["error"].is_null());

    let (status, rows) = get_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "list route must answer, got {rows}");
    let rows = rows
        .as_array()
        .expect("list is an array, not the SPA shell");
    assert_eq!(rows.len(), 1, "the row just written must come back");
    assert_eq!(rows[0]["response"]["url"], "http://x/orders");
}

/// 没人发过请求的 endpoint 回 `[]`，不是 `404`。
///
/// 这是历史条最常发的请求——每个首次被打开的 endpoint 都会来一次——而「全新 endpoint」
/// 是空历史而非缺失资源。回 `404` 会让两者在两条 transport 上都无法区分。
#[tokio::test]
async fn an_endpoint_with_no_history_reads_as_empty() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    let (status, body) = get_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "got {body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0));
}

/// 列表收窄到自己的 endpoint，故兄弟的行不能漏进来。renderer 按 `endpointId` 建键
/// 而不逐行复核，这只有在这个读取足够窄的前提下才成立。
#[tokio::test]
async fn the_list_never_includes_a_sibling_endpoints_rows() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    post_json(addr, "/api/response-history", append_body()).await;
    let mut other = append_body();
    other["endpointId"] = json!("e2");
    let (status, entry) = post_json(addr, "/api/response-history", other).await;
    assert_eq!(status, 201, "got {entry}");

    let (_, rows) = get_json(addr, "/api/response-history/e1").await;
    let rows = rows.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert!(
        rows.iter().all(|r| r["endpointId"] == "e1"),
        "a project-wide read leaked a sibling's row: {rows:?}"
    );
}

/// 按 id 的 GET 与 DELETE 共用一条路径，集合级的 POST 必须与它们在 radix tree 里共存。
/// `removed` 正是这个删除不像邻路由那样回 `204` 的全部理由，故连计数一起断言。
#[tokio::test]
async fn clear_answers_how_many_rows_went() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    post_json(addr, "/api/response-history", append_body()).await;

    let (status, body) = delete_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "clear route must answer, got {body}");
    assert_eq!(body["removed"], 1);

    // 幂等：第二次清除没东西可删，如实说 0，而不是对一个仍活着的 endpoint 回 404。
    let (status, body) = delete_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "clearing twice is not an error, got {body}");
    assert_eq!(body["removed"], 0);

    let (_, rows) = get_json(addr, "/api/response-history/e1").await;
    assert_eq!(rows.as_array().map(Vec::len), Some(0));
}

/// 上限由 `db::response_history::append` 强制而非在此——但这是 transport 层面的证据：
/// 十一次 POST 只剩十行，即每个写入方都走了裁剪路径。
#[tokio::test]
async fn the_per_endpoint_cap_holds_across_transport_appends() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    for _ in 0..11 {
        let (status, body) = post_json(addr, "/api/response-history", append_body()).await;
        assert_eq!(status, 201, "every append must answer, got {body}");
    }

    let (_, rows) = get_json(addr, "/api/response-history/e1").await;
    assert_eq!(
        rows.as_array().map(Vec::len),
        Some(crate::domain::RESPONSE_HISTORY_KEEP_DEFAULT as usize),
        "the db write path trims; the transport must not reintroduce rows"
    );
}

/// `PUT` 一个 JSON body。
async fn put_json(
    addr: SocketAddr,
    path: &str,
    body: serde_json::Value,
) -> (reqwest::StatusCode, serde_json::Value) {
    let res = client()
        .put(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes");
    let status = res.status();
    let body = res.json().await.unwrap_or(serde_json::Value::Null);
    (status, body)
}

/// `favorite` 与 `keep` 是**字面量**段，与 `/api/response-history/{endpointId}` 在
/// axum 的 radix tree 的同一层。抢优先级失败的那条会落到 SPA fallback，回 `200` 加
/// 一个 HTML body——在 curl 看来与一个正常端点毫无区别。故这里读的是 body。
#[tokio::test]
async fn the_literal_routes_win_over_the_parameter_route() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    let (status, entry) = post_json(addr, "/api/response-history", append_body()).await;
    assert_eq!(status, 201, "got {entry}");
    let row_id = entry["id"].as_str().expect("row id").to_string();

    // 收藏：请求体点名历史行的 id，回裸布尔值。
    let (status, body) = put_json(
        addr,
        "/api/response-history/favorite",
        json!({ "id": row_id, "favorite": true }),
    )
    .await;
    assert_eq!(status, 200, "the favorite route must answer, got {body}");
    assert_eq!(
        body,
        json!(true),
        "it must answer a boolean, not the SPA shell"
    );

    // 保留条数：GET 读、PUT 改，回裸数字。
    let (status, body) = get_json(addr, "/api/response-history/keep").await;
    assert_eq!(status, 200, "the keep route must answer, got {body}");
    assert_eq!(body, json!(crate::domain::RESPONSE_HISTORY_KEEP_DEFAULT));

    let (status, body) = put_json(addr, "/api/response-history/keep", json!({ "keep": 3 })).await;
    assert_eq!(status, 200, "the keep route must answer, got {body}");
    assert_eq!(body, json!(3));

    let (_, body) = get_json(addr, "/api/response-history/keep").await;
    assert_eq!(
        body,
        json!(3),
        "the new count must be the one the db trims with"
    );
}

/// 收藏的行不参与裁剪：超过条数之后它仍在，且带着收藏标记。
#[tokio::test]
async fn a_favorited_row_is_still_there_after_the_trim() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    put_json(addr, "/api/response-history/keep", json!({ "keep": 2 })).await;

    let (_, first) = post_json(addr, "/api/response-history", append_body()).await;
    let row_id = first["id"].as_str().expect("row id").to_string();
    put_json(
        addr,
        "/api/response-history/favorite",
        json!({ "id": row_id, "favorite": true }),
    )
    .await;

    for _ in 0..5 {
        post_json(addr, "/api/response-history", append_body()).await;
    }

    let (_, rows) = get_json(addr, "/api/response-history/e1").await;
    let rows = rows.as_array().expect("array");
    assert_eq!(rows.len(), 3, "the favorite plus the two newest rows");
    assert!(
        rows.iter()
            .any(|r| r["id"] == row_id && r["isFavorite"] == true),
        "the favorited row must survive with its flag: {rows:?}"
    );
}

/// 请求快照随响应一起落库：响应区左侧画的就是它。
#[tokio::test]
async fn the_request_snapshot_travels_with_the_response() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    let mut body = append_body();
    body["request"] = json!({
        "method": "POST",
        "url": "http://x/orders?page=2",
        "headers": [["content-type", "application/json"]],
        "body": "{\"a\":1}"
    });
    let (status, entry) = post_json(addr, "/api/response-history", body).await;
    assert_eq!(status, 201, "got {entry}");
    assert_eq!(entry["request"]["url"], "http://x/orders?page=2");

    let (_, rows) = get_json(addr, "/api/response-history/e1").await;
    let rows = rows.as_array().expect("array");
    assert_eq!(rows[0]["request"]["method"], "POST");
    assert_eq!(rows[0]["request"]["body"], "{\"a\":1}");
}
