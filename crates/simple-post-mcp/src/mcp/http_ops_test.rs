use super::*;
use crate::mcp::{arg_bool_opt, arg_str, arg_str_opt, arg_u64_opt};
use simple_post_core::domain::Environment;
use std::collections::HashMap;

#[test]
fn table_has_two_rows() {
    assert_eq!(table().len(), 2);
}

#[test]
fn flatten_object_to_pairs() {
    let v = json!({ "X-Foo": "bar", "X-Num": 42 });
    let pairs = flatten_string_map(Some(&v), "h").unwrap();
    assert_eq!(
        pairs,
        vec![
            ("X-Foo".into(), "bar".into()),
            ("X-Num".into(), "42".into())
        ]
    );
}

#[test]
fn flatten_array_of_pairs() {
    let v = json!([["X-A", "1"], ["X-B", "2"]]);
    let pairs = flatten_string_map(Some(&v), "h").unwrap();
    assert_eq!(pairs.len(), 2);
}

#[test]
fn flatten_rejects_bad_shape() {
    let v = json!("not a map");
    assert!(flatten_string_map(Some(&v), "h").is_err());
}

#[test]
fn settings_override_extracts_values() {
    let so = json!({
        "timeout_ms": 5000u64,
        "follow_redirects": false,
        "max_response_size_kb": 1024u64,
    });
    let args = RunRequestArgs {
        method: "GET".into(),
        url: "/x".into(),
        headers: None,
        params: None,
        body: None,
        auth: None,
        env_id: None,
        base_url_definition_id: None,
        module_id: None,
        settings_override: Some(so),
        timeout_ms: None,
        follow_redirects: None,
        max_response_size_kb: None,
    };
    let (t, r, m) = extract_settings(&args.settings_override, &args);
    assert_eq!(t, Some(5000));
    assert_eq!(r, Some(false));
    assert_eq!(m, Some(1024));
}

#[test]
fn arg_helpers_cover_top_level() {
    let v = json!({ "method": "GET", "url": "/x", "n": 7 });
    assert_eq!(arg_str(&v, "method").unwrap(), "GET");
    assert_eq!(arg_str(&v, "url").unwrap(), "/x");
    assert_eq!(arg_u64_opt(&v, "n"), Some(7));
    assert!(!arg_bool_opt(&v, "missing"));
    assert_eq!(arg_str_opt(&v, "missing"), None);
}

#[test]
fn apply_base_url_handles_env_and_override() {
    let mut module_base_urls = HashMap::new();
    module_base_urls.insert(
        "auth".into(),
        HashMap::from([("mod_a".into(), "https://auth.example.com".into())]),
    );
    let env = Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        module_base_urls,
        variables: HashMap::new(),
        is_active: true,
    };
    let (joined, base) = apply_base_url("/login", Some(&env), Some("auth"), Some("mod_a"));
    assert_eq!(joined, "https://auth.example.com/login");
    assert_eq!(base, "https://auth.example.com");

    let (passthru, _) = apply_base_url("/login", None, None, None);
    assert_eq!(passthru, "/login");

    // 没有模块就没有格子：同一个服务只有 `mod_a` 有 URL，且不向它或别的服务兜底。
    let (no_module, empty) = apply_base_url("/login", Some(&env), Some("auth"), None);
    assert_eq!(no_module, "/login");
    assert_eq!(empty, "");
}

#[test]
fn parse_method_known_and_custom() {
    assert_eq!(
        simple_post_core::domain::parse_http_method("get").unwrap(),
        HttpMethod::Get
    );
    assert_eq!(
        simple_post_core::domain::parse_http_method("POST").unwrap(),
        HttpMethod::Post
    );
    assert!(matches!(
        simple_post_core::domain::parse_http_method("FROBNICATE").unwrap(),
        HttpMethod::Custom(_)
    ));
}

#[test]
fn header_pairs_round_trip() {
    let pairs = [[String::from("a"), String::from("b")]];
    let tuples = header_pairs_to_tuples(&pairs);
    assert_eq!(tuples, vec![("a".into(), "b".into())]);
}

// =========================================================================
// oauth2 —— 取 token 与注入请求的端到端
// =========================================================================
//
// 下面这些用两个真实的 loopback socket（一个假 IdP、一个假 API）走完
// `api_call_endpoint` 的整条路径。纯单元断言看不出「token 到底有没有进到真正发出去的
// 那个请求里」，而那正是这一层唯一值得断言的东西。

/// 回固定响应的假服务端，并把**每一次**收到的请求按顺序记下。
///
/// 收多次是必要的：「命中缓存」那条要数「第二次调用有没有再回 IdP」，而一个只
/// `accept` 一次的 listener 会在第二次连接时直接 connection refused，把断言变成一个
/// 测不到目标行为的连接错误。
struct Stub {
    url: String,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    _handle: tokio::task::JoinHandle<()>,
}

async fn stub(status_line: &'static str, content_type: &'static str, body: &'static str) -> Stub {
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
                "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
    /// 到此刻为止收到的请求条数。测试中途读它，才不会和被测代码抢同一个连接。
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
        "simple-post-mcp-oauth2-test-{}-{}-{}-{id}",
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

/// 项目 + 模块 + 一个带 oauth2 鉴权的端点，且项目已授予 `Full`
/// （`api_call_endpoint` 要 Gate B 放行）。返回端点 id。
fn oauth2_endpoint(
    st: &AppState,
    token_url: &str,
    api_url: &str,
    token_location: simple_post_core::domain::OAuth2TokenLocation,
) -> String {
    use simple_post_core::db::{nodes, permissions, projects};
    use simple_post_core::domain::{
        AuthConfig, AuthType, EndpointDetail, HttpMethod, OAuth2Auth, OAuth2ClientAuth,
        OAuth2GrantType, Project, RequestBody, ResourceNode, ResourceNodeType,
    };
    use simple_post_core::permissions::Permission;

    let project = Project {
        id: "p_oauth".into(),
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
        node_id: "e_oauth".into(),
        method: HttpMethod::Get,
        url: format!("{api_url}/me"),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig {
            kind: AuthType::Oauth2,
            oauth2: Some(OAuth2Auth {
                grant_type: OAuth2GrantType::ClientCredentials,
                token_url: token_url.to_string(),
                client_id: "cid".into(),
                client_secret: "csecret".into(),
                client_auth: OAuth2ClientAuth::Basic,
                extra_params: vec![],
                token_location,
                ..OAuth2Auth::default()
            }),
            ..AuthConfig::default()
        },
        docs: None,
        notes: None,
        tags: vec![],
    };
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &project)?;
        nodes::create(
            c,
            &node("p_oauth", "m_oauth", ResourceNodeType::Module, None),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_oauth",
                "e_oauth",
                ResourceNodeType::Endpoint,
                Some("m_oauth"),
            ),
            Some(&detail),
        )?;
        permissions::set(c, "p_oauth", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    "e_oauth".to_string()
}

#[tokio::test]
async fn api_call_endpoint_injects_a_freshly_fetched_oauth2_token_into_the_header() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-E2E","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("header");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        OAuth2TokenLocation::Header,
    );

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    assert_eq!(idp.count(), 1, "第一次调用应当换一次 token");
    let token_request = idp.nth(0);
    assert!(
        token_request.contains("grant_type=client_credentials"),
        "got: {token_request}"
    );

    assert_eq!(api.count(), 1);
    let api_request = api.nth(0);
    assert!(
        api_request.starts_with("GET /me HTTP/1.1"),
        "got: {api_request}"
    );
    assert!(
        api_request.contains("authorization: Bearer AT-E2E"),
        "取到的 token 必须进到真正发出的请求里，got: {api_request}"
    );
}

#[tokio::test]
async fn api_call_endpoint_puts_the_token_in_the_query_when_configured_to() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-Q","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("query");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        OAuth2TokenLocation::Query,
    );

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let api_request = api.nth(0);
    assert!(
        api_request.starts_with("GET /me?access_token=AT-Q HTTP/1.1"),
        "query 档按 RFC 6749 §8.3 只带裸 access_token，got: {api_request}"
    );
    assert!(
        !api_request.to_lowercase().contains("authorization:"),
        "query 档不该再发 Authorization 头，got: {api_request}"
    );
}

/// token 端点报错时**不碰业务端点**。发出去一条裸奔请求而用户以为它带鉴权，比一次
/// 响亮的失败坏得多。
#[tokio::test]
async fn a_failing_token_endpoint_stops_the_call_before_the_api_is_touched() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "401 Unauthorized",
        "application/json",
        r#"{"error":"invalid_client","error_description":"bad secret"}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("fail");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        OAuth2TokenLocation::Header,
    );

    let err = tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect_err("取 token 失败必须让整次调用失败");
    let msg = err.to_string();
    assert!(msg.contains("invalid_client"), "got: {msg}");
    assert_eq!(api.count(), 0, "取不到 token 时业务端点不该收到任何东西");
}

/// 第二次调用命中缓存，不再回 IdP —— 「按需刷新」不打扰用户的另一半。
#[tokio::test]
async fn a_second_call_reuses_the_cached_token() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-CACHE","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("cache");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        OAuth2TokenLocation::Header,
    );

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("first call");
    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("second call");

    assert_eq!(idp.count(), 1, "命中缓存时不该再回 IdP");
    assert_eq!(api.count(), 2);
    assert!(
        api.nth(1).contains("authorization: Bearer AT-CACHE"),
        "got: {}",
        api.nth(1)
    );
}

/// 用户手写过 `Authorization` 时不覆盖它 —— 服务端看到的是 header 列表而非覆盖，
/// 两个 `Authorization` 是用户的配置被静默忽略。
#[tokio::test]
async fn a_hand_written_authorization_header_survives_the_injection() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-DROP","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("manual-auth");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        OAuth2TokenLocation::Header,
    );
    simple_post_core::db::with_tx(&st.pool, |c| {
        c.execute(
            "UPDATE endpoint_details SET headers_json = ?1 WHERE node_id = 'e_oauth'",
            [headers_json(&[simple_post_core::domain::KeyValue {
                id: "h1".into(),
                key: "Authorization".into(),
                value: "Bearer USER-WROTE-THIS".into(),
                enabled: true,
                description: None,
            }])],
        )?;
        Ok(())
    })
    .unwrap();

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    let api_request = api.nth(0);
    assert!(
        api_request.contains("authorization: Bearer USER-WROTE-THIS"),
        "got: {api_request}"
    );
    assert!(
        !api_request.contains("AT-DROP"),
        "已有 Authorization 时不该再追加一条，got: {api_request}"
    );
}

/// 用户把鉴权从 oauth2 切成 bearer 之后，那份 oauth2 载荷仍在（`resolve_auth` 对未
/// 选中的方案原样保留），但**不应**再有人去取 token。
///
/// query 档是这条的刀刃：header 档会被 `build_headers` 已写好的 `Authorization` 挡住，
/// 而 query 档不挡——一个用户已经不再选择的 token 会被拼进 URL 发出去。
#[tokio::test]
async fn an_unselected_oauth2_payload_never_reaches_the_token_endpoint() {
    use simple_post_core::domain::OAuth2TokenLocation;

    let idp = stub(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-SHOULD-NOT-APPEAR","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let api = stub("200 OK", "application/json", r#"{"ok":true}"#).await;

    let st = mcp_state("unselected");
    let id = oauth2_endpoint(
        &st,
        &format!("{}/oauth/token", idp.url),
        &api.url,
        // query 档：注入不做重名检查，是唯一能把未选中的 token 送出去的形状。
        OAuth2TokenLocation::Query,
    );

    // 把 kind 改成 bearer，同时保留那份 oauth2 载荷。
    simple_post_core::db::with_tx(&st.pool, |c| {
        c.execute(
            "UPDATE endpoint_details SET auth_json = ?1 WHERE node_id = 'e_oauth'",
            [r#"{"kind":"bearer","bearer":"USER-TOKEN","oauth2":{"grantType":"client_credentials","tokenUrl":"http://127.0.0.1:1/token","clientId":"cid","tokenLocation":"query"}}"#],
        )?;
        Ok(())
    })
    .unwrap();

    tool_api_call_endpoint(&st, json!({ "endpoint_id": id }))
        .await
        .expect("call");

    assert_eq!(idp.count(), 0, "未选中的 oauth2 载荷不该触发取 token");
    let api_request = api.nth(0);
    assert!(
        !api_request.contains("AT-SHOULD-NOT-APPEAR"),
        "未选中的 token 绝不能拼进 URL，got: {api_request}"
    );
    assert!(
        api_request.contains("authorization: Bearer USER-TOKEN"),
        "选中的 bearer 方案不受影响，got: {api_request}"
    );
}

/// `endpoint_details.headers_json` 的编码与 `db::endpoint_details::update` 一致，
/// 这里直接借它，免得测试里再抄一份 serde 调用。
fn headers_json(rows: &[simple_post_core::domain::KeyValue]) -> String {
    serde_json::to_string(rows).unwrap()
}
