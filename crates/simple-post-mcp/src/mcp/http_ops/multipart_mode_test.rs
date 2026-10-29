//! 「有效 body mode」钉在 multipart 装配上 —— 不是端点行里存的那个。
//!
//! 这两条走的是 `api_call_endpoint` 的整条路径。纯单元断言看不出「线上那一条请求到底
//! 长什么样」，而这里要验的恰恰是线上形状：Content-Type 与 body 必须由**同一个** mode
//! 决定，否则两种错法都很安静。
//!
//! 为什么这是个坑：`auto_content_type` 对 `BodyMode::Multipart` 返回 `None`
//! （boundary 只能现生成，由发送方补），而调用方给 `overrides.body` 就能换掉 mode。
//! 发送方若回头读端点行里存的 `body.mode`，同一请求里 Content-Type 依据的 mode 与装配
//! 依据的 mode 就会分家：
//!
//! - 存 json / 覆盖 multipart → 整条请求没有 Content-Type，分片数组的 JSON 原文被当
//!   body 原样发出，服务端解析不出分片；
//! - 存 multipart / 覆盖 json → `parse_parts` 拿到非数组得到零分片，body 被换成只有收尾
//!   分隔符的壳，**调用方传的内容静默消失**，而调用照样返回成功。
//!
//! loopback socket 与 `AppState` 的搭法照 `oauth2_test.rs`，不复用——那边是 oauth2 的
//! 专用夹具。

use std::sync::Arc;
use std::sync::Mutex;

use serde_json::json;

use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::domain::{BodyMode, HttpMethod, RequestBody, ResourceNodeType};

struct Stub {
    url: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn nth(&self, i: usize) -> String {
        self.seen.lock().unwrap()[i].clone()
    }
}

/// 回固定 200，把收到的原始请求记下来。
async fn stub() -> Stub {
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
    Stub {
        url: format!("http://127.0.0.1:{port}"),
        seen,
    }
}

fn mcp_state(tag: &str) -> Arc<AppState> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-multipart-mode-{}-{}-{}-{id}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目 + 模块 + 端点，端点行里存的是 `stored`。返回端点 id。
fn seed(st: &AppState, api_url: &str, stored: RequestBody) -> String {
    use simple_post_core::db::{nodes, permissions, projects};
    use simple_post_core::domain::{AuthConfig, EndpointDetail, Project, ResourceNode};
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_mp".into(),
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
        node_id: "e_mp".into(),
        method: HttpMethod::Post,
        url: format!("{api_url}/upload"),
        environment_id: None,
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: stored,
        auth: AuthConfig::default(),
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
            &node("p_mp", "m_mp", ResourceNodeType::Module, None),
            None,
        )?;
        nodes::create(
            c,
            &node("p_mp", "e_mp", ResourceNodeType::Endpoint, Some("m_mp")),
            Some(&detail),
        )?;
        permissions::set(c, "p_mp", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    "e_mp".to_string()
}

const PARTS: &str = r#"[{"name":"f","value":"hello","filename":"a.txt"}]"#;

/// 端点行里存的是 json，`overrides.body` 换成 multipart —— 线上必须是 multipart。
///
/// 改错时这条请求**没有任何 Content-Type**，而 body 是分片数组的 JSON 原文。
#[tokio::test]
async fn an_override_to_multipart_wins_over_the_stored_json_mode() {
    let api = stub().await;
    let st = mcp_state("json-to-multipart");
    let id = seed(
        &st,
        &api.url,
        RequestBody {
            mode: BodyMode::Json,
            content: r#"{"old":true}"#.into(),
            schema: None,
        },
    );

    tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": id,
            "overrides": { "body": { "mode": "multipart", "content": PARTS } },
        }),
    )
    .await
    .expect("call");

    assert_eq!(api.count(), 1);
    let wire = api.nth(0);
    assert!(
        wire.contains("multipart/form-data; boundary="),
        "覆盖成 multipart 后线上必须是 multipart 且带 boundary：{wire}"
    );
    assert!(
        wire.contains("filename=\"a.txt\""),
        "分片必须真的成形：{wire}"
    );
}

/// 端点行里存的是 multipart，`overrides.body` 换成 json —— 线上必须是 json 且带原内容。
///
/// 改错时 `multipart::apply` 把 `build_headers` 刚写上的 `application/json` 覆盖成
/// multipart，`parse_parts` 对一个 JSON 对象得到零分片，body 被换成只有收尾分隔符的壳，
/// **调用方传的内容就这么没了，而调用返回成功**。
#[tokio::test]
async fn an_override_to_json_wins_over_the_stored_multipart_mode() {
    let api = stub().await;
    let st = mcp_state("multipart-to-json");
    let id = seed(
        &st,
        &api.url,
        RequestBody {
            mode: BodyMode::Multipart,
            content: PARTS.into(),
            schema: None,
        },
    );

    tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": id,
            "overrides": { "body": { "mode": "json", "content": "{\"new\":true}" } },
        }),
    )
    .await
    .expect("call");

    assert_eq!(api.count(), 1);
    let wire = api.nth(0);
    // 头名是 reqwest 发出的，全小写。
    let lower = wire.to_ascii_lowercase();
    assert!(
        lower.contains("content-type: application/json"),
        "覆盖成 json 后线上必须是 json：{wire}"
    );
    assert!(
        !lower.contains("multipart/form-data"),
        "不该再带 multipart 的 Content-Type：{wire}"
    );
    assert!(
        wire.contains("{\"new\":true}"),
        "调用方传的内容不能被丢掉：{wire}"
    );
}
