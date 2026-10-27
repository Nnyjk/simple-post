//! 请求脚本在 MCP 侧的端到端。
//!
//! 用真实的 loopback 服务端走完 `api_call_endpoint` 的整条路径：脚本改写请求、
//! 断言、写回环境变量、崩溃与死循环。纯单元断言看不出「脚本改的那个请求到底有没有
//! 真的发出去」，而那正是这一层唯一值得断言的东西。

use serde_json::{json, Value};

use crate::mcp::AppState;

use super::tool_api_call_endpoint;

/// 回固定响应的假服务端，并把**每一次**收到的请求按顺序记下。
struct Stub {
    url: String,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    _handle: tokio::task::JoinHandle<()>,
}

async fn stub(status_line: &'static str, body: &'static str) -> Stub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let handle = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 16 * 1024];
            let Ok(n) = sock.read(&mut buf).await else {
                continue;
            };
            sink.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    Stub {
        url: format!("http://127.0.0.1:{port}"),
        seen,
        _handle: handle,
    }
}

impl Stub {
    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn nth(&self, i: usize) -> String {
        self.seen.lock().unwrap()[i].clone()
    }
}

fn mcp_state(tag: &str) -> std::sync::Arc<AppState> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-script-test-{}-{}-{}-{id}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: std::sync::Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 项目 + 模块 + 一个带两段脚本的端点，以及一个带变量的环境。返回端点 id。
fn script_endpoint(
    st: &AppState,
    api_url: &str,
    pre_request: Option<&str>,
    test: Option<&str>,
) -> String {
    use simple_post_core::db::{environments, nodes, permissions, projects};
    use simple_post_core::domain::{
        AuthConfig, EndpointDetail, Environment, HttpMethod, Project, RequestBody, ResourceNode,
        ResourceNodeType,
    };
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_script".into(),
        name: "P".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![],
        default_base_url_definition_id: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let node = |root: &str, id: &str, kind: ResourceNodeType, parent: Option<&str>| ResourceNode {
        id: id.into(),
        root_id: root.into(),
        parent_id: parent.map(String::from),
        kind,
        name: "n".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_script".into(),
        method: HttpMethod::Get,
        url: format!("{api_url}/me"),
        environment_id: Some("env_script".into()),
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: pre_request.map(String::from),
        test_script: test.map(String::from),
    };
    let env = Environment {
        id: "env_script".into(),
        project_id: "p_script".into(),
        name: "dev".into(),
        base_urls: Default::default(),
        module_base_urls: Default::default(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: vec![],
        is_active: true,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(
            c,
            &node("p_script", "m_script", ResourceNodeType::Module, None),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_script",
                "e_script",
                ResourceNodeType::Endpoint,
                Some("m_script"),
            ),
            Some(&detail),
        )?;
        environments::create(c, &env)?;
        permissions::set(c, "p_script", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    "e_script".to_string()
}

/// 结果信封里的 JSON。
fn payload(result: &crate::mcp::CallToolResult) -> Value {
    let text = match &result.content[0] {
        crate::mcp::CallToolContent::Text { text } => text.clone(),
    };
    serde_json::from_str(&text).expect("json payload")
}

#[tokio::test]
async fn a_pre_request_script_rewrites_the_request_that_is_actually_sent() {
    let api = stub("200 OK", r#"{"ok":true}"#).await;
    let st = mcp_state("rewrite");
    let id = script_endpoint(
        &st,
        &api.url,
        Some("pm.request.url = pm.request.url + '/rewritten';"),
        None,
    );

    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    // 改写后的请求才是真正发出去的那个。
    assert_eq!(api.count(), 1);
    let request = api.nth(0);
    assert!(
        request.starts_with("GET /me/rewritten HTTP/1.1"),
        "got: {request}"
    );
    // 结果信封里的 URL 也是改写后的那份。
    let body = payload(&result);
    assert_eq!(body["url"], format!("{}/me/rewritten", api.url));
}

#[tokio::test]
async fn a_test_script_asserts_and_the_assertion_reaches_the_client() {
    let api = stub("200 OK", r#"{"id":7}"#).await;
    let st = mcp_state("assert");
    let id = script_endpoint(
        &st,
        &api.url,
        None,
        Some("pm.test('状态码是 200', function () { pm.response.to.have.status(200); });\
              pm.test('响应体有 id', function () { pm.expect(pm.response.json().id).to.equal(7); });"),
    );

    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let body = payload(&result);
    let assertions = body["_script"]["assertions"]
        .as_array()
        .expect("assertions");
    assert_eq!(assertions.len(), 2);
    assert_eq!(assertions[0]["name"], "状态码是 200");
    assert_eq!(assertions[0]["passed"], true);
    assert_eq!(assertions[1]["passed"], true);
}

#[tokio::test]
async fn a_test_script_writes_an_environment_variable_the_next_call_reads() {
    let api = stub("200 OK", r#"{"id":7}"#).await;
    let st = mcp_state("env");
    let id = script_endpoint(
        &st,
        &api.url,
        Some("pm.request.url = pm.request.url + '?token={{token}}';"),
        Some("pm.environment.set('next', 'v1');"),
    );

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    // 写回的是环境行，故下一次调用能读到。
    let env = simple_post_core::db::with_conn(&st.pool, |c| {
        simple_post_core::db::environments::get(c, "env_script")
    })
    .expect("env");
    assert_eq!(env.variables.get("next").map(String::as_str), Some("v1"));
    // 已有的变量没被抹掉。
    assert_eq!(env.variables.get("token").map(String::as_str), Some("abc"));
}

#[tokio::test]
async fn a_failing_script_does_not_change_the_outcome_of_the_request() {
    let api = stub("200 OK", r#"{"ok":true}"#).await;
    let st = mcp_state("failing");
    let id = script_endpoint(
        &st,
        &api.url,
        Some("pm.request.url = 'http://127.0.0.1:1/never'; throw new Error('boom');"),
        None,
    );

    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    // 失败的脚本的改动被丢弃：请求照原样发出去。
    assert_eq!(api.count(), 1);
    assert!(api.nth(0).starts_with("GET /me HTTP/1.1"));

    // 失败信息单独呈现，请求的成败不受影响。
    let body = payload(&result);
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], 200);
    let errors = body["_script"]["errors"].as_array().expect("errors");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["kind"], "runtime");
    assert_eq!(errors[0]["phase"], "pre");
}

#[tokio::test]
async fn an_endless_script_is_interrupted_and_the_host_survives() {
    let api = stub("200 OK", r#"{"ok":true}"#).await;
    let st = mcp_state("endless");
    let id = script_endpoint(&st, &api.url, Some("while (true) {}"), None);

    let started = std::time::Instant::now();
    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");
    let elapsed = started.elapsed();

    // 调用在毫秒级返回，而不是等脚本自己跑完。
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "死循环应被打断，实际耗时 {elapsed:?}"
    );
    let body = payload(&result);
    let errors = body["_script"]["errors"].as_array().expect("errors");
    assert_eq!(errors[0]["kind"], "timeout");
    // 宿主还活着：请求照发。
    assert_eq!(body["status"], 200);
}
