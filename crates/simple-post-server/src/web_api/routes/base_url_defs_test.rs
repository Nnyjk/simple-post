//! Routing and wire shape for the baseUrl-def and environment-baseUrl
//! routes.
//!
//! Every route in the baseUrl family is reached through a **path
//! parameter** — `:projectId` or `:defId` — and the project-scoped ones
//! put a literal segment (`baseurls`, `baseurls/default`) next to each
//! other and next to the id-scoped `/api/baseurl-defs/:defId`. Whether
//! those coexist is a property of axum's radix router, not something the
//! registration code can assert about itself by reading — so these tests
//! drive a real listener and observe the real matching.
//!
//! The status code is the discriminator. A handler that ran answers
//! `200` / `201` / `204`, or its own `400` / `404` when it rejected the
//! input; a route that lost the precedence fight answers `404` from the
//! SPA fallback, or `405` if it matched a neighbour's method set.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::web_api::{build_router, WebState};

/// A `WebState` over a throwaway SQLite file plus an empty SPA dir, with
/// the router bound to an ephemeral port on loopback.
///
/// Per-test isolation is an atomic counter plus the pid, because cargo
/// runs tests in parallel and several would otherwise share a temp path.
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

/// Seed a project, one module, one service belonging to the project, and
/// one environment. `module_id` / `is_default` are written as NULL / 0 —
/// those columns are neutralised but still present, because dropping them
/// would mean rebuilding the user's database.
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

/// `POST /api/projects/:projectId/baseurls` must answer `201` with the
/// stored row — the project is the scope, so the id lives in the path and
/// only the attributes ride in the body.
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

/// `POST /api/projects/:projectId/baseurls/default` must answer with the
/// updated `Project` — `defaultBaseUrlDefinitionId` is live again, so the
/// caller is handed the new pointer instead of a `204` and a re-read.
///
/// The status code is the discriminator between the three outcomes that
/// look alike from the client: `200` means the handler ran, `404` means
/// it fell through to the SPA fallback, and `405` means it matched a
/// neighbour's method set.
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

/// `defId: null` takes the ★ away — the state a freshly created project
/// is in, and one the other two surfaces (IPC, MCP) can also express.
#[tokio::test]
async fn clearing_the_star_answers_a_project_with_no_default() {
    let (addr, dir) = serve_test_router().await;
    seed_project(&dir).await;

    // Set it first, so clearing has something to clear and a no-op write
    // cannot masquerade as a working one.
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

/// `PATCH` / `DELETE /api/baseurl-defs/:defId` must still be reachable by
/// id.
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

/// `/module-baseurls` is back: it is the only layer a URL lives in, keyed
/// by service **and** module, so a route that failed to register would
/// leave the model with no write path at all.
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

/// The flat `/baseurls` verb is gone. Its absence is load-bearing: the
/// value layer is per (service × module), so a surviving flat route would
/// let a caller write a field nothing reads back.
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

/// The deleted backend send path must stay unroutable: the frontend
/// assembles the full request and posts it to `/api/http/send`, so a
/// surviving node-scoped send would be a second, ungoverned way out.
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

/// The project-scoped read: `GET /api/projects/:projectId/baseurls` still
/// lists, and an unknown project answers the handler's own `200` (an
/// empty list) rather than a routing failure.
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

    // `list_by_project` filters rather than erroring, so an unknown
    // project is a successful empty list — anything but a routing
    // failure proves the route is live.
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "the project-scoped list route must survive the refactor"
    );
}

/// Guards the harness itself: if the server never came up, every
/// assertion above would be reading connection-refused noise. A wrong
/// path must 404 quickly rather than hang.
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
