//! `api_call_endpoint` 的 `overrides` —— 形状、缺值与错误不许被吞掉。
//!
//! 这一组要起一个真实的 loopback 服务（端点得真的发出去，才看得到路径参数被替换
//! 成了什么），故放在 `http_ops/` 目录下，与 `run_request_test.rs` 同一批。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;

use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::db;
use simple_post_core::domain::{
    AuthConfig, EndpointDetail, HttpMethod, Permission, Project, RequestBody, ResourceNode,
    ResourceNodeType,
};

/// 收到的请求：请求路径。
type Seen = Arc<Mutex<Vec<String>>>;

/// 起一个 loopback 服务，把收到的请求路径记下来。
async fn stub() -> (String, Seen) {
    use axum::{extract::State as AxumState, routing::get, Router};

    let seen: Seen = Arc::new(Mutex::new(Vec::new()));

    async fn probe(AxumState(seen): AxumState<Seen>, uri: axum::http::Uri) -> &'static str {
        seen.lock().expect("锁").push(uri.path().to_string());
        "ok"
    }

    let router = Router::new()
        .route("/probe", get(probe))
        .route("/probe/{code}", get(probe))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}"), seen)
}

/// 每个用例一份干净的库。
fn mcp_state(tag: &str) -> Arc<AppState> {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-overrides-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目与一个端点，base URL 指向 stub。
fn seed(st: &AppState, base_url: &str) -> String {
    seed_with_url(st, base_url, "/probe/{code}")
}

/// 与 [`seed`] 相同，只是端点的 path 可以换。
fn seed_with_url(st: &AppState, base_url: &str, url: &str) -> String {
    let project = Project {
        id: "p_ovr".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_ovr".into(),
        root_id: "p_ovr".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_ovr".into(),
        method: HttpMethod::Get,
        url: url.into(),
        // 每端点自定义的 base URL 优先级最高，故直接钉住 stub 的地址：不必再为它
        // 建环境格子。
        base_url_override: Some(base_url.to_string()),
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    };
    db::with_tx(&st.pool, |c| {
        db::projects::create(c, &project)?;
        db::permissions::set(c, "p_ovr", Permission::Full)?;
        db::nodes::create(c, &module, None)?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e_ovr".into(),
                root_id: "p_ovr".into(),
                parent_id: Some("m_ovr".into()),
                kind: ResourceNodeType::Endpoint,
                name: "探针".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&detail),
        )?;
        Ok(())
    })
    .expect("seed");
    "e_ovr".into()
}

/// 形状不对的覆盖值要报错，不能被静默换成空值。
///
/// 那一层若把整份覆盖值换成空值，调用方看到的是「按端点自己那份发出去
/// 的请求」，而不是「你传的 overrides 格式不对」—— 于是覆盖值整个消失，而界面与
/// 日志里看不出任何异常。
///
/// 畸形的那一行放在 `headers` 里（不是 `pathParams`），且端点的 URL 里**没有**占位符：
/// 路径参数缺失时报的错也含「overrides」二字（它会提示调用方改用
/// `overrides.pathParams`），URL 带占位符时就分不清是「覆盖值没被收下」还是
/// 「路径参数缺值」。
#[tokio::test]
async fn a_malformed_override_is_reported_instead_of_being_dropped() {
    let (api, _seen) = stub().await;
    let st = mcp_state("bad");
    let endpoint_id = seed_with_url(&st, &api, "/probe");

    let err = tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": endpoint_id,
            // 缺 `key` 的那一行：反序列化不过。
            "overrides": { "headers": [{ "value": "42" }] }
        }),
    )
    .await
    .expect_err("形状不对的覆盖值必须报错");
    let text = err.to_string();
    assert!(
        text.contains("overrides"),
        "错误要说清是 overrides 的问题：{text}"
    );
}

/// 只要 `key` 与 `value` 的那一行就能用 —— 这正是工具描述与 JSON Schema 文档化的形状。
#[tokio::test]
async fn a_key_value_only_path_param_override_is_substituted() {
    let (api, seen) = stub().await;
    let st = mcp_state("kv");
    let endpoint_id = seed(&st, &api);

    let result = tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": endpoint_id,
            "overrides": { "pathParams": [{ "key": "code", "value": "42" }] }
        }),
    )
    .await
    .expect("调用");
    assert!(
        !serde_json::to_string(&result.content)
            .unwrap()
            .contains("is_error\":true"),
        "请求本身必须成功"
    );

    let paths = seen.lock().expect("锁").clone();
    assert_eq!(
        paths,
        vec!["/probe/42".to_string()],
        "路径参数的值必须被替换进路径本身"
    );
}
