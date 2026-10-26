//! 动态变量在 MCP 发送路径上的端到端断言。
//!
//! 起一个真实的 loopback 服务当数据源：一条动态变量声明指向它（内联请求形态），
//! 消费端点的请求体里写 `{{sns}}`，然后经 `api_call_endpoint` 发出去——服务端收到的
//! 必须是**取回来的数组**，而不是字面量 `{{sns}}`。
//!
//! 这是 issue #109 的主干验收项：「Agent 建定义 → 调用消费端点 → 值自动就位」。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;

use crate::mcp::{dispatch, AppState};
use simple_post_core::db;
use simple_post_core::domain::Permission;
use simple_post_core::permissions::CapabilityPolicy;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 起一个 loopback 服务：`GET /points` 回站点清单，`POST /consume` 记下收到的 body。
///
/// 记下来的 body 放进 `Arc<Mutex<Option<String>>>`，测试在请求发完后读它。
async fn serve() -> (String, Arc<Mutex<Option<String>>>) {
    use axum::{
        body::Bytes,
        extract::State as AxumState,
        routing::{get, post},
        Json, Router,
    };

    let received: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    async fn points() -> Json<serde_json::Value> {
        Json(json!([{ "code": "A001" }]))
    }
    async fn consume(
        AxumState(received): AxumState<Arc<Mutex<Option<String>>>>,
        body: Bytes,
    ) -> &'static str {
        *received.lock().expect("锁") = Some(String::from_utf8_lossy(&body).to_string());
        "ok"
    }

    let router = Router::new()
        .route("/points", get(points))
        .route("/consume", post(consume))
        .with_state(received.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}"), received)
}

/// 一个含项目、消费端点与一条动态变量定义的库。
fn test_state(base_url: &str) -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-dynamic-send-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = simple_post_core::db::init(&dir).unwrap();
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &simple_post_core::domain::Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        db::permissions::set(c, "p1", Permission::Full)?;
        db::preferences::set_mcp_policy(c, CapabilityPolicy::FullAccess)?;
        // 消费端点：body 里写 `{{sns}}`。
        c.execute_batch(
            "INSERT INTO nodes (id, root_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e1', 'p1', 'endpoint', '消费端点', 0, 1, 1)",
        )?;
        db::endpoint_details::create(
            c,
            &simple_post_core::domain::EndpointDetail {
                node_id: "e1".into(),
                method: simple_post_core::domain::HttpMethod::Post,
                url: "/consume".into(),
                environment_id: None,
                base_url_definition_id: None,
                base_url_override: Some(base_url.to_string()),
                params: vec![],
                path_params: vec![],
                headers: vec![],
                body: simple_post_core::domain::RequestBody {
                    mode: simple_post_core::domain::BodyMode::Json,
                    content: "{\"sns\": {{sns}}}".into(),
                    schema: None,
                },
                auth: simple_post_core::domain::AuthConfig::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
            },
        )?;
        Ok(())
    })
    .expect("seed");
    Arc::new(AppState {
        pool,
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

#[tokio::test]
async fn the_consumer_request_carries_the_fetched_value() {
    let (base_url, received) = serve().await;
    let state = test_state(&base_url);

    // 先声明动态变量：数据源是那个 loopback 服务的 /points。
    let created = dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "project_id": "p1",
            "name": "sns",
            "source_inline_json": json!({
                "method": "GET",
                "url": format!("{base_url}/points"),
                "headers": [],
                "body": { "mode": "none", "content": "" },
                "auth": { "kind": "none" }
            })
            .to_string(),
        }),
    )
    .await
    .expect("建定义");

    // 再调用消费端点：body 里的 `{{sns}}` 应当已经被取回的数组替掉。
    let called = dispatch(&state, "api_call_endpoint", json!({ "endpoint_id": "e1" }))
        .await
        .expect("调用消费端点");
    let text = serde_json::to_value(&called).expect("序列化")["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(text.contains("200"), "调用应当成功：{text}");

    let body = received.lock().expect("锁").clone().expect("收到过请求");
    assert!(
        body.contains("A001"),
        "消费端点收到的 body 里应当是取回的数组：{body}"
    );
    assert!(
        !body.contains("{{sns}}"),
        "未解析的标记不该原样发出：{body}"
    );
    let _ = created;
}
