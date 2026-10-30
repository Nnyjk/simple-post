//! `api_run_request` 的端到端断言：临时请求也走端点那套解析规则。
//!
//! 这条工具要实现工具描述承诺的 `params` / `auth`，而不是只读 `headers` 与 `body`。
//! 本文件用真实 socket 钉住它现在发出去的是什么：变量替换、base URL 前置、
//! 查询参数与鉴权头。

use std::sync::Arc;
use std::sync::Mutex;

use serde_json::json;

use super::tool_api_run_request;
use crate::mcp::AppState;
use simple_post_core::domain::{Environment, ModuleBaseUrl, Project, ResourceNodeType};

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
        "simple-post-mcp-run-request-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目、一个模块与一个环境，环境的 (环境 × 模块) 格子填了 `base_url`
/// （测试里是本地 stub 的地址，故请求不会跑到外网去）。
/// 返回 (project_id, env_id, module_id)。
fn seed(st: &AppState, base_url: &str) -> (String, String, String) {
    use simple_post_core::db::{environments, nodes, projects};
    use simple_post_core::domain::ResourceNode;

    let project = Project {
        id: "p_run".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_run".into(),
        root_id: "p_run".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let env = Environment {
        id: "env_run".into(),
        project_id: "p_run".into(),
        name: "dev".into(),
        module_base_urls: [(
            "m_run".to_string(),
            ModuleBaseUrl {
                url: base_url.to_string(),
                proxy: None,
            },
        )]
        .into_iter()
        .collect(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: vec![],
        is_active: true,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(c, &module, None)?;
        environments::create(c, &env)?;
        Ok(())
    })
    .unwrap();
    ("p_run".into(), "env_run".into(), "m_run".into())
}

/// 临时请求要按端点那套规则组装：base URL 前置、变量替换、查询参数、鉴权派生的头。
#[tokio::test]
async fn a_run_request_gets_the_same_treatment_as_a_stored_endpoint() {
    let (api, seen) = stub().await;
    let st = mcp_state("full");
    let (_, env_id, module_id) = seed(&st, &api);

    let result = tool_api_run_request(
        &st,
        json!({
            "method": "GET",
            "url": "/users/{{token}}",
            "env_id": env_id,
            "module_id": module_id,
            "params": [{ "key": "page", "value": "1" }],
            "headers": [{ "key": "X-Trace", "value": "t1" }],
            "auth": { "kind": "bearer", "bearer": "tok-{{token}}" },
        }),
    )
    .await
    .expect("call");
    assert!(
        !serde_json::to_string(&result.content)
            .unwrap()
            .contains("is_error\":true"),
        "请求本身必须成功"
    );

    let wire = seen.lock().unwrap()[0].clone();
    // base URL 前置 + 变量替换 + 查询参数 —— 全部按端点那套规则。
    assert!(
        wire.contains("GET /users/abc?page=1 HTTP/1.1"),
        "临时请求必须与端点一样地拼 URL：{wire}"
    );
    // 鉴权派生的头。
    assert!(
        wire.to_ascii_lowercase()
            .contains("authorization: bearer tok-abc"),
        "auth 必须派生出 Authorization 头：{wire}"
    );
    // 用户自己写的头。
    assert!(
        wire.to_ascii_lowercase().contains("x-trace: t1"),
        "用户写的头必须发出去：{wire}"
    );
}

#[tokio::test]
async fn a_stale_environment_id_degrades_instead_of_failing_the_call() {
    // 调用方手里可能留着一个已经被删掉的环境 id。这不该让整次调用失败 ——
    // 降级成「这次请求不做变量替换」，与 `api_call_endpoint` 同一套规则
    // （`mcp::resolve_env`）。
    let st = mcp_state("stale-env");
    let outcome = tool_api_run_request(
        &st,
        json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/x",
            "env_id": "env_already_gone",
        }),
    )
    .await;
    assert!(outcome.is_ok(), "环境已被删不该让整次调用失败：{outcome:?}");
}

/// 解析不到的变量要报进结果，调用方才知道 URL 里留着 `{{...}}`。
#[tokio::test]
async fn a_run_request_reports_the_variables_it_could_not_resolve() {
    let (api, seen) = stub().await;
    let st = mcp_state("missing");
    let (_, env_id, module_id) = seed(&st, &api);

    let result = tool_api_run_request(
        &st,
        json!({
            "method": "GET",
            "url": "/x",
            "env_id": env_id,
            "module_id": module_id,
            "params": [{ "key": "q", "value": "{{nothing}}" }],
        }),
    )
    .await
    .expect("call");

    let text = serde_json::to_string(&result.content).unwrap();
    assert!(text.contains("nothing"), "解析不到的变量必须报出来：{text}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}
