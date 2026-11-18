//! 协议是端点的属性，必须一路走到线上：存库的 `GET` + GraphQL 发出去是 `POST`，
//! SSE / WebSocket 在权限门之后、出网之前被拒。

use std::sync::Arc;
use std::sync::Mutex;

use serde_json::json;

use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::domain::{
    AuthConfig, EndpointDetail, Environment, GraphQlRequest, HttpMethod, ModuleBaseUrl, Permission,
    Project, RequestBody, RequestProtocol, ResourceNodeType,
};
use simple_post_core::error::AppError;

/// 回固定 200，把收到的原始请求记下来。
async fn stub() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 16 * 1024];
            let Ok(n) = sock.read(&mut buf).await else {
                continue;
            };
            sink.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let body = r#"{"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    (format!("http://127.0.0.1:{port}"), seen)
}

fn mcp_state(tag: &str) -> Arc<AppState> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-protocol-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
    })
}

/// 种一个项目、模块、环境与一个协议为 `protocol` 的端点。
/// `permission` 是这个项目授予 MCP 的级别；`api_call_endpoint` 至少要 `ReadCall`。
fn seed(
    st: &AppState,
    base_url: &str,
    protocol: RequestProtocol,
    graphql: Option<GraphQlRequest>,
    permission: Permission,
) -> String {
    use simple_post_core::db::{environments, nodes, permissions, projects};
    use simple_post_core::domain::ResourceNode;

    let project = Project {
        id: "p_proto".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_proto".into(),
        root_id: "p_proto".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_proto".into(),
        // 故意存 GET：GraphQL 的固定 POST 必须来自协议，而不是调用方传的方法字符串。
        method: HttpMethod::Get,
        url: "/graphql".into(),
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody {
            mode: simple_post_core::domain::BodyMode::Json,
            // 切换协议前那份 http 载荷，带一个不该出现在报告里的变量。
            content: r#"{"legacy":"{{ghost}}"}"#.into(),
            schema: None,
        },
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
        responses: vec![],
        protocol,
        graphql,
    };
    let env = Environment {
        id: "env_proto".into(),
        project_id: "p_proto".into(),
        name: "dev".into(),
        module_base_urls: [(
            "m_proto".to_string(),
            ModuleBaseUrl {
                url: base_url.to_string(),
                rewrite: None,
            },
        )]
        .into_iter()
        .collect(),
        variables: [("userId".to_string(), "42".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: vec![],
        created_at: 0,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(c, &module, None)?;
        nodes::create(
            c,
            &ResourceNode {
                id: "e_proto".into(),
                root_id: "p_proto".into(),
                parent_id: Some("m_proto".into()),
                kind: ResourceNodeType::Endpoint,
                name: "e".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&detail),
        )?;
        environments::create(c, &env)?;
        permissions::set(c, "p_proto", permission)?;
        Ok(())
    })
    .unwrap();
    "e_proto".to_string()
}

/// 存库的 GET 在 GraphQL 协议下必须发成 POST，正文由 query / variables 组装。
#[tokio::test]
async fn a_stored_graphql_endpoint_with_get_method_is_sent_as_post() {
    let (api, seen) = stub().await;
    let st = mcp_state("graphql");
    let id = seed(
        &st,
        &api,
        RequestProtocol::GraphQl,
        Some(GraphQlRequest {
            query: "{ user(id: {{userId}}) { id } }".into(),
            variables: r#"{"b":2,"a":1}"#.into(),
            operation_name: None,
        }),
        Permission::Full,
    );

    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let wire = seen.lock().unwrap()[0].clone();
    assert!(
        wire.contains("POST /graphql HTTP/1.1"),
        "线上动词必须来自 resolve 后的 method（固定 POST），而不是存库的 GET：{wire}"
    );
    let body = wire.split("\r\n\r\n").nth(1).unwrap_or_default();
    assert_eq!(
        body, "{\"query\":\"{ user(id: 42) { id } }\",\"variables\":{\"a\":1,\"b\":2}}",
        "正文必须由 GraphQL 投影组装：{wire}"
    );
    assert!(
        wire.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "GraphQL 必须带 JSON Content-Type：{wire}"
    );
    // 切换协议前那份 http 载荷的缺失变量不该混进这次请求的报告。
    let envelope = serde_json::to_string(&result.content).unwrap();
    assert!(
        !envelope.contains("ghost"),
        "GraphQL 不跑 HTTP body 替换，那份载荷的缺失变量不该被报出来：{envelope}"
    );
}

/// 权限检查在协议拦截之前：无权限者不该通过错误文案探测端点是不是 SSE。
#[tokio::test]
async fn an_unauthorized_caller_learns_nothing_about_the_protocol() {
    let (api, _seen) = stub().await;
    let st = mcp_state("sse-denied");
    let id = seed(&st, &api, RequestProtocol::Sse, None, Permission::Read);

    let outcome = tool_api_call_endpoint(&st, json!({ "endpoint_id": id })).await;

    assert!(
        matches!(&outcome, Err(AppError::PermissionDenied(_))),
        "权限必须先于协议检查：{outcome:?}"
    );
}

/// 有权限时 SSE 仍要在出网之前被拒。
#[tokio::test]
async fn an_authorized_sse_endpoint_is_rejected_before_any_request() {
    let (api, seen) = stub().await;
    let st = mcp_state("sse-full");
    let id = seed(&st, &api, RequestProtocol::Sse, None, Permission::Full);

    let outcome = tool_api_call_endpoint(&st, json!({ "endpoint_id": id })).await;

    assert!(
        matches!(&outcome, Err(AppError::Validation(m)) if m.contains("该端点协议是 sse")),
        "SSE 必须在出网之前被拒：{outcome:?}"
    );
    assert!(seen.lock().unwrap().is_empty(), "被拒的调用不该出网");
}
