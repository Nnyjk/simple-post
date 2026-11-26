//! `POST /api/http/send` 带脚本的真实请求。
//!
//! 跑真实 listener + 真实上游：脚本改写后的请求必须**真的**发出去，而不只是回包
//! 里写着。线形状（`{ http, script }`）也在这里钉住——它与 MCP 端的结果信封同源，
//! 桌面端与浏览器端读的是同一个形状。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web_api::{build_router, WebState};

/// 建一个 `WebState`：一次性的 SQLite 文件 + 空 SPA 目录，router 绑在 loopback 上一个
/// 临时端口。
async fn serve_test_router() -> (SocketAddr, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);

    let dir = std::env::temp_dir().join(format!(
        "simple-post-web-http-send-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");

    let spa_dir = dir.join("spa");
    std::fs::create_dir_all(&spa_dir).expect("spa dir");

    let pool = crate::db::init(&dir).expect("db init");
    let state = WebState {
        pool,
        daemon: crate::daemon_supervisor::McpDaemonHandle::new(),
        data_dir: dir.clone(),
        password: None,
        base_path: None,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let router = build_router(state, spa_dir);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (addr, dir)
}

/// 回固定响应的假上游，并把收到的请求行记下来。
async fn upstream() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8 * 1024];
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

/// 种下一个项目、一个模块、一个带前置脚本的端点。端点的 URL 指向假上游。
fn seed(dir: &std::path::Path, api_url: &str) {
    use crate::domain::{
        AuthConfig, EndpointDetail, HttpMethod, Project, RequestBody, ResourceNode,
        ResourceNodeType,
    };

    let pool = crate::db::init(dir).expect("db init");
    let project = Project {
        id: "p1".into(),
        name: "demo".into(),
        description: None,
        color: "#000".into(),
        created_at: 1,
        updated_at: 1,
    };
    let node = |id: &str, kind: ResourceNodeType, parent: Option<&str>| ResourceNode {
        id: id.into(),
        root_id: "p1".into(),
        parent_id: parent.map(String::from),
        kind,
        name: id.into(),
        description: None,
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    };
    let detail = EndpointDetail {
        node_id: "ep_a".into(),
        method: HttpMethod::Get,
        url: format!("{api_url}/me"),
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        responses: vec![],
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: Some("pm.request.url = pm.request.url + '/rewritten';".into()),
        test_script: None,
        tls: None,
        protocol: Default::default(),
        graphql: None,
    };
    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(c, &project)?;
        crate::db::nodes::create(c, &node("mod_a", ResourceNodeType::Module, None), None)?;
        crate::db::nodes::create(
            c,
            &node("ep_a", ResourceNodeType::Endpoint, Some("mod_a")),
            Some(&detail),
        )?;
        Ok(())
    })
    .expect("seed tree");
}

async fn post_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> serde_json::Value {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .json(&body)
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);
    response.json().await.expect("json body")
}

#[tokio::test]
async fn a_send_without_an_endpoint_has_no_script_section() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir, "");

    let body = post_json(
        addr,
        "/api/http/send",
        serde_json::json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/never",
            "headers": [],
            "body": null,
            "timeoutMs": 1000,
            "followRedirects": true,
            "maxResponseSizeKb": 1024
        }),
    )
    .await;

    // 传输层的信封在 `http` 那一半；没有端点就没有脚本链。
    assert!(body["http"].is_object());
    assert!(body.get("script").is_none());
}

#[tokio::test]
async fn a_send_with_an_endpoint_runs_the_scripts_and_reports_them() {
    let (api_url, seen) = upstream().await;
    let (addr, dir) = serve_test_router().await;
    seed(&dir, &api_url);

    let body = post_json(
        addr,
        "/api/http/send",
        serde_json::json!({
            "method": "GET",
            "url": format!("{api_url}/me"),
            "headers": [],
            "body": null,
            "timeoutMs": 5000,
            "followRedirects": true,
            "maxResponseSizeKb": 1024,
            "endpointId": "ep_a",
            "env": { "id": null, "variables": {} }
        }),
    )
    .await;

    // 脚本改写后的请求才是真正发出去的那个。
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /me/rewritten HTTP/1.1"),
        "got: {}",
        requests[0]
    );

    // 结果里有脚本这一段。
    assert!(body["script"].is_object());
    assert!(body["script"]["assertions"].is_array());
    assert!(body["script"]["errors"].is_array());
    // 请求本身的成败照旧。
    assert_eq!(body["http"]["response"]["status"], 200);
}

/// `POST /api/http/send-progress` 把同一份请求发出去，进度帧先到、终局最后到。
///
/// 这条流是浏览器端唯一的进度通道：`/api/http/send` 一次返回整段 JSON，读不了中间过程。
/// 故线形状必须钉住——每帧 `data:` 都是 `{type, …}`，终局那一句带着与 `/send` 同形的信封。
#[tokio::test]
async fn send_progress_streams_frames_and_ends_with_the_outcome() {
    let (api_url, seen) = upstream().await;
    let (addr, dir) = serve_test_router().await;
    seed(&dir, &api_url);

    // 有请求体就有进度帧：首帧在写出第一块之前，末帧等于总长度。体积压在假上游一次
    // `read` 能吃掉的范围里——上游只读一次就回响应，请求体太大时它会提前关连接。
    let payload = "x".repeat(4096);
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-progress"))
        .json(&serde_json::json!({
            "method": "POST",
            "url": format!("{api_url}/upload"),
            "headers": [],
            "body": payload,
            "timeoutMs": 5000,
            "followRedirects": true,
            "maxResponseSizeKb": 1024
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("text/event-stream"),
        "响应必须是事件流"
    );

    let text = response.text().await.expect("stream body");
    let frames: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| serde_json::from_str(data.trim()).expect("每帧都是 JSON"))
        .collect();

    let progress: Vec<&serde_json::Value> =
        frames.iter().filter(|f| f["type"] == "progress").collect();
    assert!(progress.len() >= 2, "首帧与末帧都该在");
    assert_eq!(
        progress[0]["progress"]["sentBytes"], 0,
        "首帧在写出第一块之前"
    );
    assert_eq!(
        progress[0]["progress"]["totalBytes"],
        payload.len() as u64,
        "分母是请求体的字节数"
    );
    assert!(
        progress
            .iter()
            .all(|f| f["progress"]["sentBytes"].is_number()),
        "每帧都带已发送字节数"
    );

    let last = frames.last().expect("至少有一帧");
    assert_eq!(last["type"], "done", "终局必须是 done");
    assert_eq!(last["outcome"]["http"]["response"]["status"], 200);
    assert!(
        last["outcome"].get("script").is_none(),
        "没传 endpointId 就没有脚本那一段"
    );

    // 请求真的发出去了。
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("POST /upload HTTP/1.1"),
        "got: {}",
        requests[0]
    );
}

/// 上游连不上时，流仍以 `done` 收尾，网络失败的描述在信封的 `http` 那一半。
///
/// 传输层的失败不是「执行层之外的错误」：`/api/http/send` 对它返回 `{ok:false,error}` 而不是
/// 5xx，这条流必须给出同一个信封，否则同一个请求在两个入口下变成两种结果。
#[tokio::test]
async fn send_progress_reports_a_transport_failure_in_the_envelope() {
    let (addr, dir) = serve_test_router().await;
    seed(&dir, "");

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/http/send-progress"))
        .json(&serde_json::json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/never",
            "headers": [],
            "body": null,
            "timeoutMs": 1000,
            "followRedirects": true,
            "maxResponseSizeKb": 1024
        }))
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);

    let text = response.text().await.expect("stream body");
    let last: serde_json::Value = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| serde_json::from_str::<serde_json::Value>(data.trim()).unwrap())
        .next_back()
        .expect("至少有一帧");
    assert_eq!(last["type"], "done");
    assert_eq!(last["outcome"]["http"]["ok"], false);
    // 连不上 loopback 上没人监听的端口：Windows 上可能先撞超时，两条都算传输层失败。
    let kind = last["outcome"]["http"]["error"]["kind"]
        .as_str()
        .unwrap_or("");
    assert!(
        kind == "network" || kind == "timeout",
        "传输层失败的种类应当是 network 或 timeout，got: {kind}"
    );
    assert!(
        !last["outcome"]["http"]["error"]["message"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "错误消息要有内容：{last}"
    );
}
