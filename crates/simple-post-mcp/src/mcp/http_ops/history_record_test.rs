//! MCP 发出的调用也要进调用历史。
//!
//! 历史条原本只收界面上发的那几次，于是 agent 通过 MCP 发的请求在历史里查不到，
//! 而 `endpoint_history` 读的就是那张表。故这条用例盯住「发一次，历史里就多一行」。

use serde_json::json;

use crate::mcp::AppState;

use super::tool_api_call_endpoint;

/// 回固定响应的假服务端。
async fn stub(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 16 * 1024];
            let _ = sock.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn mcp_state(tag: &str) -> std::sync::Arc<AppState> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-history-test-{}-{}-{}-{id}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
    })
}

/// 项目 + 模块 + 一个指向假服务端的端点。返回端点 id。
fn endpoint(st: &AppState, base_url: &str) -> String {
    use simple_post_core::db::{nodes, permissions, projects};
    use simple_post_core::domain::{
        EndpointDetail, HttpMethod, Project, RequestBody, ResourceNode, ResourceNodeType,
    };
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_hist".into(),
        name: "P".into(),
        description: None,
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
        node_id: "e_hist".into(),
        method: HttpMethod::Get,
        url: "/me".into(),
        base_url_override: Some(base_url.to_string()),
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: Default::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(
            c,
            &node("p_hist", "m_hist", ResourceNodeType::Module, None),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_hist",
                "e_hist",
                ResourceNodeType::Endpoint,
                Some("m_hist"),
            ),
            Some(&detail),
        )?;
        permissions::set(c, "p_hist", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    "e_hist".to_string()
}

/// 发一次之后，历史里就有一行：状态码、响应体与那次发出去的请求。
#[tokio::test]
async fn a_call_through_mcp_lands_in_the_call_history() {
    let base = stub(r#"{"ok":true}"#).await;
    let st = mcp_state("record");
    let id = endpoint(&st, &base);

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("调用");

    let rows = simple_post_core::db::with_conn(&st.pool, |c| {
        simple_post_core::db::response_history::list_by_endpoint(c, &id)
    })
    .expect("读历史");
    assert_eq!(rows.len(), 1, "发过一次就该有一行历史");
    assert_eq!(rows[0].response.status, 200);
    assert_eq!(rows[0].response.body, r#"{"ok":true}"#);
    let request = rows[0]
        .request
        .as_ref()
        .expect("那次发出去的请求也要存下来");
    assert_eq!(request.method, simple_post_core::domain::HttpMethod::Get);
    assert!(request.url.starts_with(&base), "got: {}", request.url);
    // 线上那一份请求头：传输层补的那几条也在里面。
    assert!(
        request
            .headers
            .iter()
            .any(|(k, _)| k == "host" || k == "accept-encoding"),
        "got: {:?}",
        request.headers
    );
}

/// 再发一次，历史里就多一行。
#[tokio::test]
async fn every_call_adds_its_own_row() {
    let base = stub(r#"{"ok":true}"#).await;
    let st = mcp_state("twice");
    let id = endpoint(&st, &base);

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("第一次");
    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("第二次");

    let rows = simple_post_core::db::with_conn(&st.pool, |c| {
        simple_post_core::db::response_history::list_by_endpoint(c, &id)
    })
    .expect("读历史");
    assert_eq!(rows.len(), 2, "两次调用就该有两行历史");
}
