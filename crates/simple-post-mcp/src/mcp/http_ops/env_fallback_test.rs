//! 端点没点名环境时的回落：MCP 侧按项目的默认环境解析。
//!
//! 端点行不再绑定环境，故 MCP 侧每次调用要么自己点名一个环境，要么不点名。不点名时
//! 回落的是**项目的默认环境**（一个项目里最早创建的那个）—— 与界面那侧一致：界面用
//! 的是它自己记住的「当前环境」，而那个环境在库里是同一个项目的某个环境。
//!
//! 回落必须有，否则同一个端点在界面发有 host、变量已替换，在 MCP 侧发却裸奔。

use std::sync::Arc;
use std::sync::Mutex;

use serde_json::json;

use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::domain::{
    AuthConfig, EndpointDetail, Environment, HttpMethod, ModuleBaseUrl, Project, RequestBody,
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

/// 每个用例一份干净的库。
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

/// 种一个项目、模块、端点与一个环境。那个环境是这个项目里最早创建的，故它是默认环境。
fn seed(st: &AppState, base_url: &str) -> String {
    use simple_post_core::db::{environments, nodes, permissions, projects};
    use simple_post_core::domain::ResourceNode;
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_env".into(),
        name: "P".into(),
        description: None,
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
        base_url_override: None,
        params: vec![],
        path_params: vec![],
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
        tls: None,
    };
    let env = Environment {
        id: "env_env".into(),
        project_id: "p_env".into(),
        name: "dev".into(),
        module_base_urls: [(
            "m_env".to_string(),
            ModuleBaseUrl {
                url: base_url.to_string(),
                proxy: None,
            },
        )]
        .into_iter()
        .collect(),
        variables: [("userId".to_string(), "42".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: vec![],
        // 这个项目里最早创建的环境 —— 故它是默认环境。
        created_at: 0,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
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

/// 端点没点名环境时，MCP 侧要按项目的默认环境解析，与界面那侧一致。
#[tokio::test]
async fn an_endpoint_without_a_named_environment_falls_back_to_the_default_one() {
    let (api, seen) = stub().await;
    let st = mcp_state("fallback");
    let id = seed(&st, &api);

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let wire = seen.lock().unwrap()[0].clone();
    // base URL 来自 (环境 × 模块) 的格子，变量也由那个环境替换。
    assert!(
        wire.contains("GET /users/42 HTTP/1.1"),
        "端点没点名环境时必须按项目默认的环境解析：{wire}"
    );
}
