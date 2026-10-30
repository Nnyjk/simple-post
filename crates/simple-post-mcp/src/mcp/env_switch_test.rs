//! 每次调用自己点名环境 —— MCP 侧的端到端验证。
//!
//! 端点行不再绑定环境，故「用哪个环境解析」是**每次调用**自己决定的：不点名就用项目
//! 的默认环境（最早创建的那个），点名了就用点名的那个。
//!
//! 这条用例走的是 MCP 的 dispatch 表（`api_call_endpoint` 按名字取），与客户端真实
//! 调用同一条路，故它验的是「点名 + 解析」合起来的结果，而不是某一层单独的行为。
//!
//! 它守住的正是用户报告的那件事：**换了环境，实际端口要跟着换**。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::mcp::AppState;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 一台回固定 200 的假服务器，把收到的原始请求记下来。
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
fn state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-env-switch-{}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 按名字取一个工具函数 —— 与 `dispatch.rs` 走的是同一张表。
fn tool(name: &str) -> crate::mcp::ToolFn {
    crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("tool {name} must be registered"))
        .func
}

/// 种一个项目、模块与端点，外加两个环境：dev 建得早（它是默认环境）、staging 建得晚。
///
/// 端点行上没有环境绑定 —— 它不绑定环境，用哪个环境由每次调用点名。
fn seed(st: &AppState, dev_url: &str, staging_url: &str) {
    use simple_post_core::db::{environments, nodes, permissions, projects};
    use simple_post_core::domain::{
        AuthConfig, EndpointDetail, Environment, HttpMethod, ModuleBaseUrl, Project, RequestBody,
        ResourceNode, ResourceNodeType,
    };
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_switch".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_switch".into(),
        root_id: "p_switch".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_switch".into(),
        method: HttpMethod::Get,
        url: "/ping".into(),
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
    // `created_at` 决定谁是默认环境：dev 建得早，故它是默认环境。
    let env = |id: &str, name: &str, url: &str, created_at: i64| Environment {
        id: id.into(),
        project_id: "p_switch".into(),
        name: name.into(),
        module_base_urls: [(
            "m_switch".to_string(),
            ModuleBaseUrl {
                url: url.to_string(),
                proxy: None,
            },
        )]
        .into_iter()
        .collect(),
        variables: Default::default(),
        disabled_variables: vec![],
        created_at,
    };

    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(c, &module, None)?;
        nodes::create(
            c,
            &ResourceNode {
                id: "e_switch".into(),
                root_id: "p_switch".into(),
                parent_id: Some("m_switch".into()),
                kind: ResourceNodeType::Endpoint,
                name: "e".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&detail),
        )?;
        environments::create(c, &env("env_dev", "dev", dev_url, 1_700_000_000_000))?;
        environments::create(
            c,
            &env("env_staging", "staging", staging_url, 1_700_000_100_000),
        )?;
        permissions::set(c, "p_switch", Permission::Full)?;
        Ok(())
    })
    .unwrap();
}

/// 不点名环境时，请求打在项目的默认环境（最早创建的那个）上。
#[tokio::test]
async fn an_unnamed_environment_falls_back_to_the_project_default() {
    let (dev_api, dev_seen) = stub().await;
    let (staging_api, staging_seen) = stub().await;
    let st = state();
    seed(&st, &dev_api, &staging_api);

    (tool("api_call_endpoint"))(&st, json!({ "endpoint_id": "e_switch" }))
        .await
        .expect("call");

    assert_eq!(dev_seen.lock().unwrap().len(), 1, "默认环境是 dev");
    assert_eq!(staging_seen.lock().unwrap().len(), 0);
}

/// 点名另一个环境时，请求打到那个环境的地址上 —— 这就是「切换环境，端口跟着换」。
#[tokio::test]
async fn naming_another_environment_sends_the_request_to_that_environment() {
    let (dev_api, dev_seen) = stub().await;
    let (staging_api, staging_seen) = stub().await;
    let st = state();
    seed(&st, &dev_api, &staging_api);

    (tool("api_call_endpoint"))(
        &st,
        json!({ "endpoint_id": "e_switch", "environment_id": "env_staging" }),
    )
    .await
    .expect("call with a named environment");

    assert_eq!(
        dev_seen.lock().unwrap().len(),
        0,
        "点名了 staging 就不该再打 dev 的地址"
    );
    assert_eq!(
        staging_seen.lock().unwrap().len(),
        1,
        "请求要打在点名环境的地址上"
    );
}

/// 同一次会话里两次调用可以走两个环境：当前环境不是落库的状态，故上一次调用不决定
/// 下一次用哪个。
#[tokio::test]
async fn two_calls_in_one_session_can_use_two_different_environments() {
    let (dev_api, dev_seen) = stub().await;
    let (staging_api, staging_seen) = stub().await;
    let st = state();
    seed(&st, &dev_api, &staging_api);

    (tool("api_call_endpoint"))(
        &st,
        json!({ "endpoint_id": "e_switch", "environment_id": "env_staging" }),
    )
    .await
    .expect("call with staging");
    (tool("api_call_endpoint"))(&st, json!({ "endpoint_id": "e_switch" }))
        .await
        .expect("call without a named environment");

    assert_eq!(staging_seen.lock().unwrap().len(), 1);
    assert_eq!(
        dev_seen.lock().unwrap().len(),
        1,
        "不点名的那次走默认环境，与上一次点名无关"
    );
}
