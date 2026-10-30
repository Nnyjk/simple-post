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

/// 收到的请求：请求路径与 body。
type Received = Arc<Mutex<Option<(String, String)>>>;

/// 起一个 loopback 服务：`GET /points` 回站点清单，`POST /consume` 记下收到的路径与 body。
///
/// 记下来的东西放进 `Arc<Mutex<Option<(String, String)>>>`，测试在请求发完后读它：
/// 第一项是请求路径，第二项是 body。
async fn serve() -> (String, Received) {
    use axum::{
        body::Bytes,
        extract::State as AxumState,
        routing::{get, post},
        Json, Router,
    };

    let received: Received = Arc::new(Mutex::new(None));

    async fn points() -> Json<serde_json::Value> {
        Json(json!([{ "code": "A001" }]))
    }
    async fn consume(
        AxumState(received): AxumState<Received>,
        uri: axum::http::Uri,
        body: Bytes,
    ) -> &'static str {
        *received.lock().expect("锁") = Some((
            uri.path().to_string(),
            String::from_utf8_lossy(&body).to_string(),
        ));
        "ok"
    }

    let router = Router::new()
        .route("/points", get(points))
        .route("/consume", post(consume))
        // 路径参数那一档：路径里带一个 `{code}` 段。
        .route("/consume/{code}", post(consume))
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
    test_state_with(base_url, "/consume", "{\"sns\": {{sns}}}")
}

/// 与 [`test_state`] 相同，只是端点的 path 与 body 可以换。
fn test_state_with(base_url: &str, url: &str, body_content: &str) -> Arc<AppState> {
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
                url: url.into(),
                environment_id: None,
                base_url_override: Some(base_url.to_string()),
                params: vec![],
                path_params: vec![],
                headers: vec![],
                body: simple_post_core::domain::RequestBody {
                    mode: simple_post_core::domain::BodyMode::Json,
                    content: body_content.into(),
                    schema: None,
                },
                auth: simple_post_core::domain::AuthConfig::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
                tls: None,
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

    let (_, body) = received.lock().expect("锁").clone().expect("收到过请求");
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

/// 两个项目：p1 有消费端点，p2 是数据源所在的项目。
///
/// 两个项目一开始都给 `Full`——建定义是一次写操作，它落在数据源接口所属的项目上，
/// 故数据源项目的权限要够写才能建出来。建完之后测试再把 p2 降到目标档位，于是
/// 「取数时 p2 只有只读 / 许发请求」这件事才成立。
fn two_project_state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-dynamic-gate-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = simple_post_core::db::init(&dir).unwrap();
    db::with_tx(&pool, |c| {
        for (project_id, project_name) in [("p1", "First"), ("p2", "Second")] {
            db::projects::create(
                c,
                &simple_post_core::domain::Project {
                    id: project_id.into(),
                    name: project_name.into(),
                    description: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )?;
        }
        db::permissions::set(c, "p1", Permission::Full)?;
        db::permissions::set(c, "p2", Permission::Full)?;
        db::preferences::set_mcp_policy(c, CapabilityPolicy::FullAccess)?;

        // p1 的消费端点：body 里写 `{{sns}}`。
        c.execute_batch(
            "INSERT INTO nodes (id, root_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e1', 'p1', 'endpoint', '消费端点', 0, 1, 1);\
             INSERT INTO nodes (id, root_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e2', 'p2', 'endpoint', '数据源接口', 0, 1, 1)",
        )?;
        db::endpoint_details::create(
            c,
            &simple_post_core::domain::EndpointDetail {
                node_id: "e1".into(),
                method: simple_post_core::domain::HttpMethod::Post,
                url: "http://127.0.0.1:9/consume".into(),
                environment_id: None,
                base_url_override: Some("http://127.0.0.1:9".into()),
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
                tls: None,
            },
        )?;
        db::endpoint_details::create(
            c,
            &simple_post_core::domain::EndpointDetail {
                node_id: "e2".into(),
                method: simple_post_core::domain::HttpMethod::Get,
                url: "http://127.0.0.1:9/points".into(),
                environment_id: None,
                base_url_override: Some("http://127.0.0.1:9".into()),
                params: vec![],
                path_params: vec![],
                headers: vec![],
                body: simple_post_core::domain::RequestBody::default(),
                auth: simple_post_core::domain::AuthConfig::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
                tls: None,
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
async fn a_send_cannot_borrow_another_projects_data_source() {
    // 定义挂在消费方项目（p1）名下，但数据源接口可以是另一个项目（p2）的。取数用的是
    // **p2** 的 base URL 与鉴权，故 p2 的权限决定这次取数能不能做。少了这道门，一个只读
    // 项目的客户端就能借本项目的接口把别的项目的凭据用出去。
    //
    // 定义自己挂在 p1（消费方项目）名下，数据源接口在 p2 —— 数据源可跨项目，
    // 但取数花的是 p2 的凭据，故门看的是 p2。
    let state = two_project_state();
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "project_id": "p1", "name": "sns", "source_endpoint_id": "e2" }),
    )
    .await
    .expect("建定义");
    // 建完之后把数据源项目降到只读：它不许发请求，故拿它的数据源去取数应当被门挡下。
    db::with_tx(&state.pool, |c| {
        db::permissions::set(c, "p2", Permission::Read)
    })
    .unwrap();

    let err = dispatch(&state, "api_call_endpoint", json!({ "endpoint_id": "e1" }))
        .await
        .expect_err("数据源项目只读，不该能借它的凭据取数");
    assert!(err.to_string().contains("permission denied"), "{err}");
}

#[tokio::test]
async fn a_send_may_use_a_data_source_whose_project_is_read_call() {
    // 上一条的对照：数据源项目给到 `ReadCall`（许发请求）时，同一条路应当走通。
    let state = two_project_state();
    dispatch(
        &state,
        "dynamic_var_create",
        json!({ "project_id": "p1", "name": "sns", "source_endpoint_id": "e2" }),
    )
    .await
    .expect("建定义");
    db::with_tx(&state.pool, |c| {
        db::permissions::set(c, "p2", Permission::ReadCall)
    })
    .unwrap();

    // 取数会真的发出去站请求（127.0.0.1:9 连不上），故这里只验「门没有拦住」：
    // 报的不是 permission denied。失败有两种形状——权限不足是 `Err`，网络失败是
    // 一个带错误文本的信封——故两种都收下，只看文案里有没有 permission denied。
    let outcome = dispatch(&state, "api_call_endpoint", json!({ "endpoint_id": "e1" })).await;
    let rendered = match &outcome {
        Ok(result) => serde_json::to_value(result).expect("序列化")["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        Err(err) => err.to_string(),
    };
    assert!(
        !rendered.contains("permission denied"),
        "门不该拦住：{rendered}"
    );
}

/// 路径参数值里的 `{{sns}}` 也要被取数并替换。
///
/// 路径参数的值在替换时同样会解 `{{var}}`，故扫描「这个请求引用了哪些动态变量」时
/// 必须把它算进去 —— 不算的话这个变量永远不会被取数，替换后仍是字面量。
///
/// **body 里不能出现 `{{sns}}`**：body 里出现的话，变量名照样会被扫到，fetch 照样发生，
/// 于是这条用例在「没扫路径参数」的实现下也会通过 —— 它证明不了自己。
#[tokio::test]
async fn a_path_parameter_value_carries_the_fetched_value() {
    let (base_url, received) = serve().await;
    let state = test_state_with(&base_url, "/consume/{code}", "{\"x\": 1}");

    // 定义挂在 p1 名下（内联请求没有别的项目可依），权限由归属项目决定。
    dispatch(
        &state,
        "dynamic_var_create",
        json!({
            "project_id": "p1",
            "name": "sns",
            "extract_path": "$[0].code",
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

    // 调用时把路径参数的值交给它 —— 值里写的是动态变量的标记。
    let called = dispatch(
        &state,
        "api_call_endpoint",
        json!({
            "endpoint_id": "e1",
            "overrides": { "pathParams": [{ "key": "code", "value": "{{sns}}" }] }
        }),
    )
    .await
    .expect("调用消费端点");
    let text = serde_json::to_value(&called).expect("序列化")["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(text.contains("200"), "调用应当成功：{text}");

    let (path, _) = received.lock().expect("锁").clone().expect("收到过请求");
    assert_eq!(
        path, "/consume/A001",
        "路径参数的值应当被取回的动态变量替掉"
    );
}
