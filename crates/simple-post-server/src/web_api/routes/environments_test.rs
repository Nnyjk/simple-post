//! 环境变量的停用 / 启用路由的路由与线格式。
//!
//! 这条路由与它旁边的 `/vars` 与 `/vars/rename` 相邻，而 `/vars/disabled` 与
//! `/vars/rename` 在 radix tree 上是同一层的两个静态段。它们能否共存是 axum 的性质，
//! 不是注册代码读一读就能断言的——所以这里驱动真实 listener，观察真实的匹配结果。
//!
//! 判别依据是状态码。跑起来的 handler 回 `200`，或它自己拒收输入时的 `400`；抢优先级
//! 失败的路由会从 SPA fallback 回 `404`，或在被邻居的方法集匹配上时回 `405`。

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
        "simple-post-web-env-routing-{}-{id}",
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

/// 种一个项目与一个带变量的环境。
fn seed_project_with_env(dir: &std::path::Path) {
    let pool = crate::db::init(dir).expect("db init");
    crate::db::with_tx(&pool, |c| {
        c.execute_batch(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
             VALUES ('p1', 'demo', '#000', 1, 1)",
        )?;
        c.execute_batch(
            "INSERT INTO environments \
             (id, project_id, name, created_at, variables_json) \
             VALUES ('env_1', 'p1', 'dev', 1, '{\"token\":\"abc\"}')",
        )?;
        Ok(())
    })
    .expect("seed project");
}

/// 停用一个变量：回 `200` 与停用后的环境，且值还在。
#[tokio::test]
async fn the_disable_route_answers_and_keeps_the_value() {
    let (addr, dir) = serve_test_router().await;
    seed_project_with_env(&dir);

    let response = post_json(
        addr,
        "/api/environments/env_1/vars/disabled",
        serde_json::json!({ "key": "token", "disabled": true }),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "expected the disable handler to run (200), not a routing failure"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(
        body["disabledVariables"],
        serde_json::json!(["token"]),
        "停用的变量名必须回到前端"
    );
    assert_eq!(
        body["variables"]["token"], "abc",
        "停用不删值：照样存、照样显示，只是不参与替换"
    );
}

/// 启用是同一条路由的另一个方向。
#[tokio::test]
async fn the_same_route_enables_a_variable_again() {
    let (addr, dir) = serve_test_router().await;
    seed_project_with_env(&dir);

    post_json(
        addr,
        "/api/environments/env_1/vars/disabled",
        serde_json::json!({ "key": "token", "disabled": true }),
    )
    .await;
    let response = post_json(
        addr,
        "/api/environments/env_1/vars/disabled",
        serde_json::json!({ "key": "token", "disabled": false }),
    )
    .await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["disabledVariables"], serde_json::json!([]));
}

/// 停用一个不存在的变量必须是 `400`，不能静默成功。
#[tokio::test]
async fn disabling_a_variable_that_does_not_exist_is_rejected() {
    let (addr, dir) = serve_test_router().await;
    seed_project_with_env(&dir);

    let response = post_json(
        addr,
        "/api/environments/env_1/vars/disabled",
        serde_json::json!({ "key": "ghost", "disabled": true }),
    )
    .await;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "静默成功会让调用方以为它生效了"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["code"], "validation");
}

/// 新建环境回传**落库之后**的那一行：创建时刻是它在库里认「默认环境」用的字段。
///
/// 回传 0 的话，界面手里的创建时刻与库里那一行对不上：界面按 0 排「最早创建的那个」，
/// 后端按真实时刻排，两侧于是给出两个不同的默认环境——接口预览的请求与真正发出去的
/// 请求不是同一个。
#[tokio::test]
async fn creating_an_env_returns_the_stored_creation_time() {
    let (addr, dir) = serve_test_router().await;
    seed_project_with_env(&dir);

    let response = post_json(
        addr,
        "/api/environments",
        serde_json::json!({ "projectId": "p1", "name": "staging" }),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: serde_json::Value = response.json().await.expect("json body");
    let created_at = body["createdAt"].as_i64().expect("createdAt is a number");
    assert!(
        created_at > 0,
        "创建时刻不能是 0：库里补的是当前时刻，回传 0 会让界面与库算出两个默认环境"
    );

    // 库里那一行与回传的那一份必须是同一个数。
    let pool = crate::db::init(&dir).expect("db init");
    let stored = crate::db::with_conn(&pool, |c| {
        crate::db::environments::get(c, body["id"].as_str().unwrap())
    })
    .expect("env row");
    assert_eq!(
        stored.created_at, created_at,
        "回传的那一份与库里那一行必须是同一个创建时刻"
    );

    // 默认环境 = 最早创建的那个。`env_1` 的创建时刻是 1，比新建的早，故它仍是默认环境。
    let default = crate::db::with_conn(&pool, |c| crate::db::environments::get_default(c, "p1"))
        .expect("default env")
        .expect("the project has a default env");
    assert_eq!(
        default.id, "env_1",
        "最早创建的那个才是默认环境——新环境排在它后面"
    );
}
