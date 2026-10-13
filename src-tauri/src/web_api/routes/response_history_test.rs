//! Routing and wire shape for the response-history routes.
//!
//! Driven against a real listener for the reason `base_url_defs_test.rs`
//! gives: whether `/api/response-history` and
//! `/api/response-history/:endpointId` coexist in axum's radix tree is a
//! property of the router, not something the registration code can assert
//! about itself by reading. The failure mode is nasty because it is
//! silent — a route that lost the precedence fight falls through to the
//! SPA fallback and answers `200` with an HTML body, which to `curl` looks
//! exactly like a working endpoint that happens to ignore its path.
//!
//! The discriminator used throughout is therefore the *body*: these tests
//! read the JSON back rather than only checking the status code.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// A `WebState` over a throwaway SQLite file plus an empty SPA dir, with
/// the router bound to an ephemeral port on loopback.
///
/// Copied from `base_url_defs_test.rs` rather than shared: the counter is
/// per-module, and a shared harness across two `#[path]` test modules
/// would need a `pub(crate)` home neither of them owns.
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

/// Seed one project holding one module and one endpoint under it.
///
/// The endpoint is what `response_history.endpoint_id` references, and the
/// `nodes.root_id` column it denormalises is what the project-scoped list
/// joins through — so an endpoint seeded without a root would make the
/// list legitimately empty and the append would fail its foreign key,
/// which would look like a routing bug in every test below.
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

/// The body the renderer sends after folding `ApiResponse`'s header pairs
/// through `tuplesToRecord`. `bodyJson` is omitted entirely — the domain
/// type carries `skip_serializing_if`, so a response the caller never
/// parsed is a shape without that key, and the route has to accept it.
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

/// `POST` then `GET` — the round trip the strip depends on, and the one
/// that fails loudly if the append route was never registered (the GET
/// would answer the SPA shell's `200` and this would compare HTML to a
/// list).
#[tokio::test]
async fn append_then_list_answers_both_routes() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    let (status, entry) = post_json(addr, "/api/response-history", append_body()).await;
    assert_eq!(status, 201, "append route must answer, got {entry}");
    assert_eq!(entry["endpointId"], "e1");
    assert_eq!(entry["response"]["status"], 200);
    // The server mints both of these; the caller's body carried neither.
    assert!(entry["id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(entry["createdAt"].as_i64().is_some_and(|n| n > 0));
    // Defaults a renderer that omits them must not have to send.
    assert_eq!(entry["bodyTruncated"], false);
    assert!(entry["response"]["error"].is_null());

    let (status, rows) = get_json(addr, "/api/response-history?rootId=p1").await;
    assert_eq!(status, 200, "list route must answer, got {rows}");
    let rows = rows
        .as_array()
        .expect("list is an array, not the SPA shell");
    assert_eq!(rows.len(), 1, "the row just written must come back");
    assert_eq!(rows[0]["response"]["url"], "http://x/orders");
}

/// `rootId` is required on this route, unlike `GET /api/nodes`. The
/// status code alone proves little here — an SPA-fallback `200` would also
/// pass a `status == 200` check — so the body is the discriminator: an
/// empty list would render as "this project has no history", which is a
/// different and wrong answer to "you did not say which project".
#[tokio::test]
async fn a_list_without_root_id_is_rejected_rather_than_answered_empty() {
    let (addr, _dir) = serve_test_router().await;

    let (status, body) = get_json(addr, "/api/response-history").await;
    assert_eq!(status, 400, "expected a validation error, got {body}");
    assert_eq!(body["code"], "validation");
}

/// The id-scoped delete and the collection-level GET/POST have to coexist
/// in the radix tree. `removed` is the whole reason this route does not
/// answer `204` like its neighbours, so the count is asserted too.
#[tokio::test]
async fn clear_answers_how_many_rows_went() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    post_json(addr, "/api/response-history", append_body()).await;

    let (status, body) = delete_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "clear route must answer, got {body}");
    assert_eq!(body["removed"], 1);

    // Idempotent: the second clear has nothing to remove and says so,
    // rather than 404-ing on an endpoint that is still alive.
    let (status, body) = delete_json(addr, "/api/response-history/e1").await;
    assert_eq!(status, 200, "clearing twice is not an error, got {body}");
    assert_eq!(body["removed"], 0);

    let (_, rows) = get_json(addr, "/api/response-history?rootId=p1").await;
    assert_eq!(rows.as_array().map(Vec::len), Some(0));
}

/// The cap is enforced in `db::response_history::append`, not here — but
/// this is the transport-level proof that eleven POSTs leave ten rows,
/// i.e. that every writer goes through the trimming path.
#[tokio::test]
async fn the_per_endpoint_cap_holds_across_transport_appends() {
    let (addr, dir) = serve_test_router().await;
    seed_tree(&dir).await;

    for _ in 0..11 {
        let (status, body) = post_json(addr, "/api/response-history", append_body()).await;
        assert_eq!(status, 201, "every append must answer, got {body}");
    }

    let (_, rows) = get_json(addr, "/api/response-history?rootId=p1").await;
    assert_eq!(
        rows.as_array().map(Vec::len),
        Some(crate::domain::RESPONSE_HISTORY_PER_ENDPOINT as usize),
        "the db write path trims; the transport must not reintroduce rows"
    );
}
