// 纯函数层：缓存键、校验、粘贴解析、form 解码、应答解析、过期判断与注入。

use oauth2::basic::{BasicErrorResponseType, BasicTokenResponse};
use oauth2::{HttpRequest, HttpResponse, StandardErrorResponse, SyncHttpClient};

use super::form::form_decode;
use super::parse::{extract_code, parse_pasted, parse_token_response};
use super::store::{is_expired, unix_secs};
use super::test_fixtures::{client_credentials, kv};
use super::token_endpoint::{client_for, decorate};
use super::*;
use crate::domain::{OAuth2Auth, OAuth2ClientAuth, OAuth2GrantType, OAuth2TokenLocation};

/// 抓下 crate 真正发出的请求、回一句 canned 200 的 sync client。
///
/// 有了它，断言就能落在**线上的字节**上，而不是某个中间结构体——协议机制归 crate
/// 之后，中间结构体已经不是本项目的东西了。
#[derive(Default)]
struct CaptureClient {
    body: std::cell::RefCell<String>,
    headers: std::cell::RefCell<Vec<(String, String)>>,
}

impl SyncHttpClient for CaptureClient {
    type Error = std::convert::Infallible;

    fn call(&self, request: HttpRequest) -> Result<HttpResponse, Self::Error> {
        *self.body.borrow_mut() = String::from_utf8_lossy(request.body()).into_owned();
        *self.headers.borrow_mut() = request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Ok(HttpResponse::new(br#"{"access_token":"AT"}"#.to_vec()))
    }
}

/// 按生产路径组出 client_credentials 请求：同一个 `client_for` + 同一个 `decorate`。
fn capture_client_credentials(cfg: &OAuth2Auth) -> (String, Vec<(String, String)>) {
    let client = client_for(cfg).expect("client_for");
    let mut r: oauth2::ClientCredentialsTokenRequest<
        '_,
        StandardErrorResponse<BasicErrorResponseType>,
        BasicTokenResponse,
    > = client.exchange_client_credentials();
    decorate!(r, cfg, scope);
    let transport = CaptureClient::default();
    // 不看应答内容，只看发出去的字节。
    let _ = r.request(&transport);
    let body = transport.body.borrow().clone();
    let headers = transport.headers.borrow().clone();
    (body, headers)
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
// form 解码与 query 档的百分号编码
// ---------------------------------------------------------------------------

#[test]
fn the_query_token_is_percent_encoded_like_the_renderer_does() {
    // query 档的 token 走 `crate::url::percent_encode`（与渲染进程的
    // `encodeURIComponent` 同字符集），不是 form 编码 —— 空格是 `%20` 而不是 `+`。
    //
    // client 凭据放 Basic 头时的 form 编码（RFC 6749 §2.3.1）是 `oauth2` crate 的
    // 职责，不在本项目里，故此处不测。
    assert_eq!(crate::url::percent_encode("Aladdin"), "Aladdin");
    assert_eq!(crate::url::percent_encode("open sesame"), "open%20sesame");
    assert_eq!(crate::url::percent_encode("a+b"), "a%2Bb");
}

#[test]
fn form_decoding_reads_the_idp_response_encoding() {
    // 少数 IdP 回的 token 应答是这个编码：`+` 是空格，`%XX` 是字节。
    assert_eq!(form_decode("a+b"), "a b");
    assert_eq!(form_decode("a%20b"), "a b");
    assert_eq!(form_decode("access_token=AT"), "access_token=AT");
    assert_eq!(form_decode("中文%20%E4%B8%AD"), "中文 中");
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
    let (form, headers) = capture_client_credentials(&cfg);
    assert!(form.contains("client_id=cid"), "form={form}");
    assert!(form.contains("client_secret=csecret"), "form={form}");
    assert!(
        form.contains("audience=https%3A%2F%2Fapi.example.com"),
        "form={form}"
    );
    assert!(
        !form.contains("resource"),
        "停用的 extra param 不该发出：{form}"
    );
    // 凭据放 body 时不该再出现在 Authorization 头里。
    assert!(
        !headers.iter().any(|(k, _)| k == "authorization"),
        "headers={headers:?}"
    );
}

#[test]
fn client_secret_basic_form_encodes_before_base64() {
    // RFC 6749 §2.3.1：id 与 secret 先各做一次 form 编码，**再**把 `id:secret` 拼起来
    // 做 base64。故 secret 里的空格在 header 里是 `+`（`QWxhZGRpbjpvcGVuK3Nlc2FtZQ==`），
    // 而 RFC 7617 Basic 的例子（`QWxhZGRpbjpvcGVuIHNlc2FtZQ==`）编码的是**未**做 form
    // 编码的原文——直接 base64 原文会在 secret 含 `:` / `+` / 空格时被服务端解出另
    // 一组凭据。这条断言打在 crate 真正发出的 header 上。
    let mut cfg = client_credentials();
    cfg.client_auth = OAuth2ClientAuth::Basic;
    cfg.client_id = "Aladdin".into();
    cfg.client_secret = "open sesame".into();

    let (_, headers) = capture_client_credentials(&cfg);
    let auth = headers
        .iter()
        .find(|(k, _)| k == "authorization")
        .map(|(_, v)| v.as_str())
        .expect("Basic 档必须带 Authorization 头");
    assert_eq!(auth, "Basic QWxhZGRpbjpvcGVuK3Nlc2FtZQ==");
    // base64 解回来必须是 form 编码后的 `Aladdin:open+sesame`。
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(auth.trim_start_matches("Basic "))
            .map(|b| String::from_utf8(b).unwrap()),
        Ok("Aladdin:open+sesame".to_string())
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
