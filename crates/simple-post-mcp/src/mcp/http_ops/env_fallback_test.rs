//! 端点没绑环境时的回落：MCP 侧按项目当前激活的环境解析。
//!
//! 界面那侧用的是工作区当前环境（顶栏的环境切换会写 `environments.is_active`），
//! 而端点行上的 `environment_id` 通常是空的 —— 界面自己建的接口从不写它。故 MCP 侧
//! 必须回落，否则同一个端点在界面发有 host、变量已替换，在 MCP 侧发却裸奔。

use std::sync::Arc;
use std::sync::Mutex;

use serde_json::json;

use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::domain::{
    AuthConfig, BaseUrlDefinition, EndpointDetail, Environment, HttpMethod, Project, RequestBody,
    ResourceNodeType,
};

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
        "simple-post-mcp-env-fallback-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目、模块、端点与一个**激活**的环境。端点的 `environment_id` 是空的
/// ——界面自己建的接口就是这样。
fn seed(st: &AppState, base_url: &str) -> String {
    use simple_post_core::db::{base_url_defs, environments, nodes, permissions, projects};
    use simple_post_core::domain::ResourceNode;
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_env".into(),
        name: "P".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![BaseUrlDefinition {
            id: "svc".into(),
            name: "api".into(),
            description: None,
        }],
        default_base_url_definition_id: Some("svc".into()),
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_env".into(),
        root_id: "p_env".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_env".into(),
        method: HttpMethod::Get,
        url: "/users/{{userId}}".into(),
        // 端点没绑环境 —— 界面那侧会回落到工作区当前环境。
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody {
            mode: simple_post_core::domain::BodyMode::None,
            content: String::new(),
            schema: None,
        },
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    };
    let env = Environment {
        id: "env_env".into(),
        project_id: "p_env".into(),
        name: "dev".into(),
        base_urls: Default::default(),
        module_base_urls: [(
            "svc".to_string(),
            [("m_env".to_string(), base_url.to_string())]
                .into_iter()
                .collect(),
        )]
        .into_iter()
        .collect(),
        variables: [("userId".to_string(), "42".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: vec![],
        // 激活的环境 —— 顶栏的环境切换写的就是这一列。
        is_active: true,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        base_url_defs::create(c, &project.base_url_definitions[0], "p_env")?;
        nodes::create(c, &module, None)?;
        nodes::create(
            c,
            &ResourceNode {
                id: "e_env".into(),
                root_id: "p_env".into(),
                parent_id: Some("m_env".into()),
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
        permissions::set(c, "p_env", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    "e_env".to_string()
}

#[tokio::test]
async fn an_endpoint_without_its_own_environment_falls_back_to_the_active_one() {
    let (api, seen) = stub().await;
    let st = mcp_state("fallback");
    let id = seed(&st, &api);

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let wire = seen.lock().unwrap()[0].clone();
    // base URL 来自 (环境 × 服务 × 模块) 的格子，变量也由那个环境替换。
    assert!(
        wire.contains("GET /users/42 HTTP/1.1"),
        "端点没绑环境时必须按项目激活的环境解析：{wire}"
    );
}
