use super::*;
use crate::domain::{KeyValue, OAuth2GrantType};
use std::path::PathBuf;

fn fresh_pool() -> DbPool {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "simple-post-oauth2-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    crate::db::init(&dir).expect("init")
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        id: format!("kv_{key}"),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }
}

/// 一份能过 `validate` 的 `client_credentials` 配置。
fn client_credentials() -> OAuth2Auth {
    OAuth2Auth {
        grant_type: OAuth2GrantType::ClientCredentials,
        token_url: "https://idp.example.com/oauth/token".into(),
        client_id: "cid".into(),
        client_secret: "csecret".into(),
        scope: "read".into(),
        ..OAuth2Auth::default()
    }
}

// ---------------------------------------------------------------------------
// 缓存键
// ---------------------------------------------------------------------------

#[test]
fn the_cache_key_ignores_the_one_shot_authorization_code() {
    let mut a = client_credentials();
    a.auth_code = "code-1".into();
    let mut b = a.clone();
    b.auth_code = "code-2".into();
    // code 是一次性输入：用户贴一次新码就换一个值。若进了哈希，缓存行会在每次换码后
    // 立刻变成孤儿，而配置语义上根本没变。
    assert_eq!(auth_key(&a), auth_key(&b));
}

#[test]
fn the_cache_key_tracks_everything_that_changes_what_the_token_is_for() {
    let base = client_credentials();
    let base_key = auth_key(&base);

    let mut other = base.clone();
    other.token_url = "https://other.example.com/token".into();
    // 换了 token URL 还复用旧 token，等于把一个 scope 不同的 token 发给新的服务端。
    assert_ne!(auth_key(&other), base_key);

    let mut rotated = base.clone();
    rotated.client_secret = "new-secret".into();
    assert_ne!(auth_key(&rotated), base_key);

    let mut rescoped = base.clone();
    rescoped.scope = "write".into();
    assert_ne!(auth_key(&rescoped), base_key);

    let mut requeried = base.clone();
    requeried.extra_params = vec![kv("audience", "https://api.example.com")];
    assert_ne!(auth_key(&requeried), base_key);

    let mut retyped = base.clone();
    retyped.grant_type = OAuth2GrantType::Password;
    assert_ne!(auth_key(&retyped), base_key);
}

#[test]
fn the_cache_key_is_stable_across_calls_and_hides_the_secret() {
    let cfg = client_credentials();
    assert_eq!(auth_key(&cfg), auth_key(&cfg));
    let key = auth_key(&cfg);
    assert_eq!(key.len(), 64, "sha256 十六进制");
    assert!(
        !key.contains("csecret"),
        "主键里带着 client secret 的明文会把凭据复制到第二张表"
    );
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

#[test]
fn every_flow_rejects_its_own_missing_fields() {
    let mut cfg = OAuth2Auth {
        grant_type: OAuth2GrantType::ClientCredentials,
        ..OAuth2Auth::default()
    };
    assert!(validate(&cfg)
        .unwrap_err()
        .to_string()
        .contains("Token URL"));
    cfg.token_url = "https://idp.example.com/token".into();
    assert!(validate(&cfg)
        .unwrap_err()
        .to_string()
        .contains("Client ID"));

    let mut code = OAuth2Auth {
        grant_type: OAuth2GrantType::AuthorizationCode,
        token_url: "https://idp.example.com/token".into(),
        client_id: "cid".into(),
        ..OAuth2Auth::default()
    };
    assert!(validate(&code).unwrap_err().to_string().contains("授权码"));
    code.auth_code = "abc".into();
    validate(&code).expect("补上授权码后应通过");

    let mut ropc = OAuth2Auth {
        grant_type: OAuth2GrantType::Password,
        token_url: "https://idp.example.com/token".into(),
        client_id: "cid".into(),
        ..OAuth2Auth::default()
    };
    assert!(validate(&ropc)
        .unwrap_err()
        .to_string()
        .contains("Username"));
    ropc.username = "u".into();
    ropc.password = "p".into();
    validate(&ropc).expect("补上用户名后应通过");

    let implicit = OAuth2Auth {
        grant_type: OAuth2GrantType::Implicit,
        ..OAuth2Auth::default()
    };
    assert!(validate(&implicit)
        .unwrap_err()
        .to_string()
        .contains("Authorization URL"));
}

/// token URL 的出站策略拒绝要原样透传 —— 那是运维设的开关，用户需要看到**为什么**
/// 发不出去，而不是一句「URL 不合法」。
#[test]
fn a_token_url_outside_the_policy_is_refused_with_the_policy_reason() {
    let cfg = OAuth2Auth {
        grant_type: OAuth2GrantType::ClientCredentials,
        token_url: "file:///C:/Windows/win.ini".into(),
        client_id: "cid".into(),
        ..OAuth2Auth::default()
    };
    let err = validate(&cfg).unwrap_err().to_string();
    assert!(err.contains("only http and https"), "got: {err}");
}

// ---------------------------------------------------------------------------
// 授权码 / 粘贴文本的解析
// ---------------------------------------------------------------------------

#[test]
fn the_authorization_code_is_read_out_of_every_shape_a_user_might_paste() {
    // 整条回调 URL，code 在 query
    assert_eq!(
        extract_code("http://127.0.0.1:8080/cb?code=abc123&state=xyz"),
        "abc123"
    );
    // 回环回调常把 code 放 fragment
    assert_eq!(extract_code("http://127.0.0.1:8080/cb#code=frag1"), "frag1");
    // `code=` 前缀形式
    assert_eq!(extract_code("code=pfx%201"), "pfx 1");
    // 裸 code 原样通过
    assert_eq!(extract_code("  raw-code  "), "raw-code");
}

/// 一条**取不到 code** 的回调 URL 不能被整条当成 code 送出去：那只会换来一个更难懂的
/// 400。返回空串让 `validate` 说「需要授权码」。
#[test]
fn a_callback_url_without_a_code_yields_nothing_rather_than_the_whole_url() {
    assert_eq!(extract_code("http://127.0.0.1/cb?error=access_denied"), "");
    assert_eq!(extract_code("http://app/cb#access_token=AT"), "");
}

#[test]
fn a_pasted_implicit_redirect_yields_its_fragment_fields() {
    let parsed = parse_pasted("http://app/cb#access_token=AT-1&token_type=Bearer&expires_in=3600")
        .expect("应解析出 fragment 里的 token");
    assert_eq!(parsed.access_token, "AT-1");
    assert_eq!(parsed.token_type, "Bearer");
    assert_eq!(parsed.expires_in, Some(3600));
}

#[test]
fn a_pasted_query_token_and_a_bare_token_both_work() {
    let q = parse_pasted("http://app/cb?access_token=AT-2").unwrap();
    assert_eq!(q.access_token, "AT-2");
    assert_eq!(q.token_type, "Bearer");

    let bare = parse_pasted("  sk-live-1234  ").unwrap();
    assert_eq!(bare.access_token, "sk-live-1234");
    assert_eq!(bare.token_type, "Bearer");
    assert_eq!(bare.expires_in, None);
}

#[test]
fn an_empty_paste_is_an_error_not_an_empty_token() {
    assert!(parse_pasted("   ").is_err());
}

// ---------------------------------------------------------------------------
// form 编解码
// ---------------------------------------------------------------------------

#[test]
fn form_encoding_runs_before_base64_for_http_basic_client_credentials() {
    // RFC 6749 §2.3.1：id 与 secret 先各做一次
    // `application/x-www-form-urlencoded` 编码，**再**把 `id:secret` 拼起来做
    // base64。故 secret 里的空格在 header 里是 `+`（`QWxhZGRpbjpvcGVuK3Nlc2FtZQ==`），
    // 而 RFC 7617 的 Basic 例子（`QWxhZGRpbjpvcGVuIHNlc2FtZQ==`）编码的是**未**做
    // form 编码的原文——直接把原文塞进 base64 会在 secret 含 `:` / `+` / 空格时
    // 被服务端解出另一组凭据。
    assert_eq!(form_encode("Aladdin"), "Aladdin");
    assert_eq!(form_encode("open sesame"), "open+sesame");
    let header = format!("{}:{}", form_encode("Aladdin"), form_encode("open sesame"));
    assert_eq!(
        crate::base64::encode(header.as_bytes()),
        "QWxhZGRpbjpvcGVuK3Nlc2FtZQ=="
    );
}

#[test]
fn form_round_trips_through_the_decoder() {
    for raw in [
        "",
        "plain",
        "with space",
        "sym+/=&?#%",
        "中文 with space",
        "a+b",
    ] {
        assert_eq!(form_decode(&form_encode(raw)), raw, "raw={raw:?}");
    }
}

#[test]
fn form_decoding_survives_a_truncated_percent_escape() {
    // 畸形输入不得 panic：粘贴框与 IdP 响应都可能给出半截转义。
    assert_eq!(form_decode("%4"), "%4");
    assert_eq!(form_decode("%zz"), "%zz");
    assert_eq!(form_decode("a%"), "a%");
}

#[test]
fn extra_params_and_client_body_auth_reach_the_form() {
    let mut cfg = client_credentials();
    cfg.client_auth = OAuth2ClientAuth::Body;
    cfg.extra_params = vec![kv("audience", "https://api.example.com"), {
        let mut d = kv("resource", "off");
        d.enabled = false;
        d
    }];
    let form = build_grant_form(&cfg);
    assert!(form.contains(&("client_id".into(), "cid".into())));
    assert!(form.contains(&("client_secret".into(), "csecret".into())));
    assert!(form.contains(&("audience".into(), "https://api.example.com".into())));
    assert!(
        !form.iter().any(|(k, _)| k == "resource"),
        "停用的 extra param 不该发出"
    );
}

// ---------------------------------------------------------------------------
// 响应解析
// ---------------------------------------------------------------------------

#[test]
fn a_json_token_response_is_understood() {
    let body = r#"{"access_token":"AT","token_type":"Bearer","expires_in":3600,"scope":"read","refresh_token":"RT"}"#;
    let t = parse_token_response(200, body, "application/json").unwrap();
    assert_eq!(t.access_token, "AT");
    assert_eq!(t.token_type, "Bearer");
    assert_eq!(t.refresh_token.as_deref(), Some("RT"));
    assert_eq!(t.scope.as_deref(), Some("read"));
    assert!(t.expires_at.unwrap() > unix_secs());
}

#[test]
fn a_form_encoded_token_response_is_also_understood() {
    // 少数 IdP 至今仍回 x-www-form-urlencoded。
    let t = parse_token_response(
        200,
        "access_token=AT&expires_in=60",
        "application/x-www-form-urlencoded",
    )
    .unwrap();
    assert_eq!(t.access_token, "AT");
    assert_eq!(
        t.token_type, "Bearer",
        "没给 token_type 时按 RFC 的默认补齐"
    );
}

#[test]
fn an_error_response_carries_the_idp_s_own_words_to_the_caller() {
    let body = r#"{"error":"invalid_client","error_description":"client authentication failed"}"#;
    let err = parse_token_response(401, body, "application/json")
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid_client"), "got: {err}");
    assert!(err.contains("client authentication failed"), "got: {err}");
}

#[test]
fn a_html_error_page_does_not_become_a_wall_of_text() {
    let body = format!("<html><body>{}</body></html>", "x".repeat(5000));
    let err = parse_token_response(502, &body, "text/html")
        .unwrap_err()
        .to_string();
    assert!(err.len() < 400, "错误信息被裁到 {}", err.len());
}

#[test]
fn a_200_without_an_access_token_is_an_error() {
    let err = parse_token_response(200, r#"{"token_type":"Bearer"}"#, "application/json")
        .unwrap_err()
        .to_string();
    assert!(err.contains("access_token"), "got: {err}");
}

#[test]
fn an_absent_expires_in_leaves_the_expiry_unknown() {
    let t = parse_token_response(200, r#"{"access_token":"AT"}"#, "application/json").unwrap();
    assert_eq!(t.expires_at, None);
    assert!(
        !is_expired(&t),
        "不知道过期时间时必须继续用着，否则每个请求都重取一次"
    );
}

// ---------------------------------------------------------------------------
// 过期判断
// ---------------------------------------------------------------------------

#[test]
fn expiry_leaves_a_skew_so_a_token_is_never_sent_already_stale() {
    let fresh = OAuth2Token {
        access_token: "AT".into(),
        refresh_token: None,
        token_type: "Bearer".into(),
        scope: None,
        expires_at: Some(unix_secs() + 3600),
        obtained_at: unix_secs(),
    };
    assert!(!is_expired(&fresh));

    let about_to_go = OAuth2Token {
        expires_at: Some(unix_secs() + 10),
        ..fresh.clone()
    };
    assert!(
        is_expired(&about_to_go),
        "还有 10 秒就到期就该算过期 —— 否则用户看到的是一次莫名 401"
    );

    let already: OAuth2Token = OAuth2Token {
        expires_at: Some(unix_secs() - 1),
        ..fresh.clone()
    };
    assert!(is_expired(&already));
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

#[test]
fn the_authorization_value_follows_rfc6749_token_type() {
    let bearer = OAuth2Token {
        access_token: "AT".into(),
        refresh_token: None,
        token_type: "Bearer".into(),
        scope: None,
        expires_at: None,
        obtained_at: 0,
    };
    assert_eq!(auth_header_value(&bearer), "Bearer AT");

    // 少数 IdP 用别的 scheme 串，替它改成 Bearer 就是打错请求。
    let mac = OAuth2Token {
        token_type: "MAC".into(),
        ..bearer.clone()
    };
    assert_eq!(auth_header_value(&mac), "MAC AT");

    // 没给 token_type 时不能留下一个前导空格。
    let bare = OAuth2Token {
        token_type: "  ".into(),
        ..bearer.clone()
    };
    assert_eq!(auth_header_value(&bare), "AT");

    // query 档只带裸 access_token，不带 scheme —— 否则 scheme 会变成另一个参数值。
    assert_eq!(auth_query_value(&bearer), "AT");
}

#[test]
fn the_token_location_decides_between_header_and_query() {
    let mut cfg = client_credentials();
    assert!(!goes_in_query(&cfg));
    cfg.token_location = OAuth2TokenLocation::Query;
    assert!(goes_in_query(&cfg));
    assert_eq!(cfg.token_param_or_default(), "access_token");
    cfg.token_param = "jwt".into();
    assert_eq!(cfg.token_param_or_default(), "jwt");
    cfg.token_param = "  ".into();
    assert_eq!(
        cfg.token_param_or_default(),
        "access_token",
        "空白参数名必须回落到默认值，否则 query 里会出现一个空 key"
    );
}

// ---------------------------------------------------------------------------
// 手动 token（implicit）
// ---------------------------------------------------------------------------

#[test]
fn manual_tokens_are_only_accepted_for_implicit() {
    let pool = fresh_pool();
    let manual = OAuth2ManualToken {
        pasted: "AT-1".into(),
    };
    let err = save_manual(&pool, &client_credentials(), &manual)
        .unwrap_err()
        .to_string();
    assert!(err.contains("implicit"), "got: {err}");
}

#[tokio::test]
async fn a_saved_implicit_token_is_reused_instead_of_refetched() {
    let pool = fresh_pool();
    let cfg = OAuth2Auth {
        grant_type: OAuth2GrantType::Implicit,
        authorization_url: "https://idp.example.com/authorize".into(),
        ..OAuth2Auth::default()
    };
    let token = save_manual(
        &pool,
        &cfg,
        &OAuth2ManualToken {
            pasted: "http://app/cb#access_token=AT-IMPLICIT&expires_in=3600".into(),
        },
    )
    .unwrap();
    assert_eq!(token.access_token, "AT-IMPLICIT");
    assert_eq!(token.token_type, "Bearer");

    // 隐式流没有 token 端点，故「按需刷新」只能是命中缓存。
    let again = ensure_token(&pool, &cfg).await.expect("命中缓存");
    assert_eq!(again.access_token, "AT-IMPLICIT");
}

#[tokio::test]
async fn implicit_without_a_saved_token_says_what_to_do_instead_of_hanging() {
    let pool = fresh_pool();
    let cfg = OAuth2Auth {
        grant_type: OAuth2GrantType::Implicit,
        authorization_url: "https://idp.example.com/authorize".into(),
        ..OAuth2Auth::default()
    };
    let err = ensure_token(&pool, &cfg).await.unwrap_err().to_string();
    assert!(err.contains("回调 URL"), "got: {err}");
}

// ---------------------------------------------------------------------------
// 真的发一次 HTTP
// ---------------------------------------------------------------------------

/// 一个只说「收下这一行」的假 IdP：把收到的请求头 + body 交回调用方，
/// 然后回一段固定响应。用来验证 form 体、`Authorization: Basic` 与响应解析
/// 在真实 socket 上成立——静态断言看不出 URL 拼错或 header 没发出去。
struct FakeIdp {
    url: String,
    handle: tokio::task::JoinHandle<String>,
}

impl FakeIdp {
    async fn start(
        status_line: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
            request
        });
        Self {
            url: format!("http://127.0.0.1:{port}/oauth/token"),
            handle,
        }
    }

    async fn received(self) -> String {
        self.handle.await.unwrap()
    }
}

#[tokio::test]
async fn a_client_credentials_grant_reaches_the_token_endpoint_over_http() {
    let idp = FakeIdp::start(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-LIVE","token_type":"Bearer","expires_in":3600}"#,
    )
    .await;
    let pool = fresh_pool();

    let mut cfg = client_credentials();
    cfg.token_url = idp.url.clone();
    // secret 里带空格，好让这条断言真的覆盖「先 form 编码再 base64」而不是
    // 恰好编码了个无特殊字符的值。
    cfg.client_secret = "open sesame".into();
    cfg.extra_params = vec![kv("audience", "https://api.example.com")];

    let token = fetch_and_store(&pool, &cfg).await.expect("取 token");
    assert_eq!(token.access_token, "AT-LIVE");

    let request = idp.received().await;
    assert!(
        request.starts_with("POST /oauth/token HTTP/1.1"),
        "got: {request}"
    );
    assert!(
        request.contains("content-type: application/x-www-form-urlencoded"),
        "got: {request}"
    );
    assert!(
        request.contains(&format!(
            "authorization: Basic {}",
            crate::base64::encode(b"cid:open+sesame")
        )),
        "got: {request}"
    );
    assert!(
        request.contains("grant_type=client_credentials"),
        "got: {request}"
    );
    assert!(
        request.contains("audience=https%3A%2F%2Fapi.example.com"),
        "额外参数应被 form 编码后发出，got: {request}"
    );
    // 走 Basic 档时凭据不进 body。
    assert!(
        !request.contains("client_secret="),
        "Basic 档的凭据只在 header 里，got: {request}"
    );

    // 取到的 token 必须落进缓存，使第二次发请求不再碰 IdP。
    let cached = ensure_token(&pool, &cfg).await.expect("命中缓存");
    assert_eq!(cached.access_token, "AT-LIVE");
}

#[tokio::test]
async fn an_expired_token_is_refreshed_with_the_refresh_grant() {
    let pool = fresh_pool();
    let idp = FakeIdp::start(
        "200 OK",
        "application/json",
        // RFC 6749 §6：刷新响应**可能**不带 refresh_token。
        r#"{"access_token":"AT-REFRESHED","token_type":"Bearer","expires_in":600}"#,
    )
    .await;

    // 缓存键含 token_url，故必须先把配置指向假 IdP 再播种过期行。
    let mut cfg = client_credentials();
    cfg.token_url = idp.url.clone();
    let key = auth_key(&cfg);
    db::with_tx(&pool, |c| {
        db::oauth2_tokens::upsert(
            c,
            &key,
            &OAuth2Token {
                access_token: "STALE".into(),
                refresh_token: Some("RT-OLD".into()),
                token_type: "Bearer".into(),
                scope: None,
                expires_at: Some(unix_secs() - 10),
                obtained_at: unix_secs() - 7200,
            },
        )
    })
    .unwrap();

    let token = ensure_token(&pool, &cfg).await.expect("走刷新");
    assert_eq!(token.access_token, "AT-REFRESHED");
    assert_eq!(
        token.refresh_token.as_deref(),
        Some("RT-OLD"),
        "刷新响应没带 refresh_token 时必须沿用旧值，否则这份配置下次的刷新机会 \
         就此消失"
    );

    let request = idp.received().await;
    assert!(
        !request.contains("grant_type=client_credentials"),
        "已有 refresh_token 时不该退回整取，got: {request}"
    );
}

/// 没有 refresh_token 的过期 token 只能整取——这是唯一能救它的办法。
#[tokio::test]
async fn an_expired_token_without_a_refresh_token_is_refetched() {
    let pool = fresh_pool();
    let idp = FakeIdp::start(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-NEW","token_type":"Bearer","expires_in":600,"refresh_token":"RT-NEW"}"#,
    )
    .await;
    let mut cfg = client_credentials();
    cfg.token_url = idp.url.clone();
    let key = auth_key(&cfg);
    db::with_tx(&pool, |c| {
        db::oauth2_tokens::upsert(
            c,
            &key,
            &OAuth2Token {
                access_token: "STALE".into(),
                refresh_token: None,
                token_type: "Bearer".into(),
                scope: None,
                expires_at: Some(unix_secs() - 10),
                obtained_at: unix_secs() - 7200,
            },
        )
    })
    .unwrap();

    let token = ensure_token(&pool, &cfg).await.expect("整取");
    assert_eq!(token.access_token, "AT-NEW");
    assert_eq!(token.refresh_token.as_deref(), Some("RT-NEW"));
    let request = idp.received().await;
    assert!(
        request.contains("grant_type=client_credentials"),
        "got: {request}"
    );
}

#[tokio::test]
async fn a_token_endpoint_error_is_reported_with_its_own_words() {
    let idp = FakeIdp::start(
        "401 Unauthorized",
        "application/json",
        r#"{"error":"invalid_client","error_description":"bad secret"}"#,
    )
    .await;
    let pool = fresh_pool();
    let mut cfg = client_credentials();
    cfg.token_url = idp.url.clone();

    let err = fetch_and_store(&pool, &cfg).await.unwrap_err().to_string();
    assert!(err.contains("invalid_client"), "got: {err}");
    assert!(err.contains("bad secret"), "got: {err}");
    assert!(
        !err.contains("AT-"),
        "错误信息里不得混进 token 本身，got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 线格式 / 入参
// ---------------------------------------------------------------------------

/// `kind` 与载荷不一致时要说清缺的是载荷，不能替调用方猜一个配置。
#[tokio::test]
async fn resolve_reports_a_half_configured_oauth2_instead_of_guessing() {
    let pool = fresh_pool();
    let err = resolve(
        &pool,
        &OAuth2TokenRequest {
            auth: AuthConfig {
                kind: AuthType::Oauth2,
                oauth2: None,
                ..AuthConfig::default()
            },
            manual: None,
            force: false,
        },
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("oauth2 载荷"), "got: {err}");
}

#[tokio::test]
async fn resolve_reports_a_non_oauth2_kind() {
    let pool = fresh_pool();
    let err = resolve(
        &pool,
        &OAuth2TokenRequest {
            auth: AuthConfig {
                kind: AuthType::Bearer,
                bearer: Some("x".into()),
                ..AuthConfig::default()
            },
            manual: None,
            force: false,
        },
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("bearer"), "got: {err}");
}

/// 命中缓存时 `refreshed` 必须是 false：UI 会用它区分「刚取到」与「两小时前的旧
/// token 还活着」，报成 true 就是一句谎话。
#[tokio::test]
async fn resolve_does_not_claim_a_cache_hit_was_a_fresh_fetch() {
    let pool = fresh_pool();
    let cfg = OAuth2Auth {
        grant_type: OAuth2GrantType::Implicit,
        authorization_url: "https://idp.example.com/authorize".into(),
        ..OAuth2Auth::default()
    };
    save_manual(
        &pool,
        &cfg,
        &OAuth2ManualToken {
            pasted: "AT-1".into(),
        },
    )
    .unwrap();

    let dto = resolve(
        &pool,
        &OAuth2TokenRequest {
            auth: AuthConfig {
                kind: AuthType::Oauth2,
                oauth2: Some(cfg),
                ..AuthConfig::default()
            },
            manual: None,
            force: false,
        },
    )
    .await
    .expect("命中缓存");
    assert_eq!(dto.access_token, "AT-1");
    assert!(!dto.refreshed, "缓存命中不是刷新");
}

#[tokio::test]
async fn a_token_endpoint_redirect_does_not_get_the_credentials_resent() {
    // 这个假 IdP 回一个 302 并把 `Location` 指到**另一个** listener：跟随重定向就会把
    // Basic 头里的 client secret 再寄一次过去。
    let (victim_url, victim_seen) = redirect_target().await;
    let idp = redirecting_idp(&victim_url).await;
    let pool = fresh_pool();
    let mut cfg = client_credentials();
    cfg.token_url = idp;

    let err = fetch_and_store(&pool, &cfg).await.unwrap_err().to_string();
    assert!(
        err.contains("302"),
        "3xx 必须当错误报出来而不是跟过去，got: {err}"
    );
    assert_eq!(victim_seen.requests(), 0, "凭据绝不能被重发到重定向目标");
}

/// 一个只记连接数的 listener：只要有人真的连上来，`requests()` 就非零。
mod redirect_target {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    pub struct Target {
        pub url: String,
        hits: Arc<AtomicUsize>,
    }

    impl Target {
        pub fn requests(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    pub async fn start() -> Target {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                use tokio::io::AsyncWriteExt;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        Target {
            url: format!("http://127.0.0.1:{port}/stolen"),
            hits,
        }
    }
}

async fn redirect_target() -> (String, redirect_target::Target) {
    let t = redirect_target::start().await;
    (t.url.clone(), t)
}

async fn redirecting_idp(location: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let location = location.to_string();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = vec![0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = sock.write_all(response.as_bytes()).await;
        let _ = sock.flush().await;
    });
    format!("http://127.0.0.1:{port}/oauth/token")
}

/// 两次并发 `ensure_token` 只应换一次 token。
///
/// 没有单飞闸时，两个并发者都看到缓存过期、都拿同一个一次性 `refresh_token` 去换；
/// 轮换 refresh token 的 IdP 会让第二次失败，而我们「刷新失败就整取」的兜底会拿已消费
/// 的授权码再换一次，用户看到一句无从排查的失败。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callers_refresh_the_expired_token_only_once() {
    let pool = fresh_pool();
    let idp = FakeIdp::start(
        "200 OK",
        "application/json",
        r#"{"access_token":"AT-ONCE","token_type":"Bearer","expires_in":600}"#,
    )
    .await;
    let mut cfg = client_credentials();
    cfg.token_url = idp.url.clone();
    let key = auth_key(&cfg);

    db::with_tx(&pool, |c| {
        db::oauth2_tokens::upsert(
            c,
            &key,
            &OAuth2Token {
                access_token: "STALE".into(),
                refresh_token: Some("RT-ONE-TIME".into()),
                token_type: "Bearer".into(),
                scope: None,
                expires_at: Some(unix_secs() - 10),
                obtained_at: unix_secs() - 7200,
            },
        )
    })
    .unwrap();

    let (a, b) = tokio::join!(ensure_token(&pool, &cfg), ensure_token(&pool, &cfg));
    assert_eq!(a.expect("first").access_token, "AT-ONCE");
    assert_eq!(b.expect("second").access_token, "AT-ONCE");
    assert_eq!(
        idp.received()
            .await
            .matches("grant_type=refresh_token")
            .count(),
        1,
        "同一个一次性 refresh_token 不该被换两次"
    );
}

/// `refresh_token` 永不跨过 Rust 进程边界。断言的是**那个键**而不是子串
/// `refresh` —— DTO 自己有个 `refreshed` 字段，用子串断言会把自己绊倒。
#[tokio::test]
async fn the_wire_dto_never_carries_a_refresh_token() {
    let dto = OAuth2TokenDto {
        access_token: "AT".into(),
        token_type: "Bearer".into(),
        expires_at: None,
        scope: None,
        refreshed: true,
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("refreshToken"), "got: {json}");
}
