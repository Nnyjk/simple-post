//! 鉴权注入与 oauth2 载荷的选择性替换。

use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, KeyValue, OAuth2Auth,
    OAuth2GrantType,
};

use super::resolve::resolve_endpoint;
use super::test_support::{empty_env, in_module_a, minimal_endpoint, set_cell, MOD_A};
// ---- auth injection ----------------------------------------------

#[test]
fn resolve_endpoint_bearer_auth_injects_authorization_header() {
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "secret-xyz".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{token}}".into()),
        basic: None,
        apikey: None,
        oauth2: None,
    };
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).unwrap();
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .expect("Authorization header must be present");
    assert_eq!(auth.1, "Bearer secret-xyz");
}

#[test]
fn resolve_endpoint_basic_auth_injects_authorization_header() {
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://api.example.com");
    env.variables.insert("u".into(), "alice".into());
    env.variables.insert("p".into(), "wonder".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.auth = AuthConfig {
        kind: AuthType::Basic,
        bearer: None,
        basic: Some(BasicAuth {
            username: "{{u}}".into(),
            password: "{{p}}".into(),
        }),
        apikey: None,
        oauth2: None,
    };
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).unwrap();
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .expect("Authorization header must be present");
    // "alice:wonder" base64 → "YWxpY2U6d29uZGVy"
    assert!(auth.1.starts_with("Basic "));
    assert!(auth.1.contains("YWxpY2U6d29uZGVy"));
}

#[test]
fn resolve_endpoint_apikey_header_injects_custom_header() {
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "ABC123".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.auth = AuthConfig {
        kind: AuthType::Apikey,
        bearer: None,
        basic: None,
        apikey: Some(ApiKeyAuth {
            key: "X-API-Key".into(),
            value: "{{k}}".into(),
            in_location: ApiKeyLocation::Header,
        }),
        oauth2: None,
    };
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).unwrap();
    let hit = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .expect("apikey header must be present");
    assert_eq!(hit.1, "ABC123");
    // URL 绝不能把 apikey 带进 query。
    assert!(!r.url.contains("ABC123"));
}

#[test]
fn resolve_endpoint_apikey_query_appends_to_url() {
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "Q-VAL".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.auth = AuthConfig {
        kind: AuthType::Apikey,
        bearer: None,
        basic: None,
        apikey: Some(ApiKeyAuth {
            key: "access_token".into(),
            value: "{{k}}".into(),
            in_location: ApiKeyLocation::Query,
        }),
        oauth2: None,
    };
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).unwrap();
    assert!(r.url.contains("access_token=Q-VAL"));
    // And there's NO Authorization header.
    assert!(r
        .headers
        .iter()
        .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")));
    // And the URL doesn't carry an apikey-shaped header.
    assert!(r
        .headers
        .iter()
        .all(|(k, _)| !k.eq_ignore_ascii_case("access_token")));
}

#[test]
fn resolve_endpoint_user_authorization_wins_over_auth_config() {
    // 用户已声明过 Authorization 头时，鉴权配置推出的那个**不能**替换它
    // （与 TS 行为一致 —— fetch 保留记录键的最后一个值，

    // 否则 cURL 会为同一个名字吐出两行 `-H`）。
    let mut env = empty_env();
    set_cell(&mut env, MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "from-env".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.headers = vec![KeyValue {
        id: "h1".into(),
        key: "Authorization".into(),
        value: "Bearer user-set".into(),
        enabled: true,
        description: None,
    }];
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{token}}".into()),
        basic: None,
        apikey: None,
        oauth2: None,
    };
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).unwrap();
    // The user's "Bearer user-set" should win.
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .unwrap();
    assert_eq!(auth.1, "Bearer user-set");
}

// ---- oauth2 ------------------------------------------------------

/// OAuth2 的配置**不在解析阶段替换**：那条规则（哪些字段被替换、哪些密钥不替换）住在
/// core 的 `simple_post_core::oauth2`（`substitute_config`），由 core 在取 token 之前
/// 替换。MCP 侧把变量池交给 `ensure_token` 即可。
///
/// 密钥那半边不是省事。替换发生在 core 内部，故 `clientSecret` / `password` /
/// `authCode` 里的 `{{var}}` 不会被解出来，而解出来的那份配置也不会进任何被回显的
/// 结构体 —— `api_resolve_endpoint` 回显的 `auth` 就是这份未替换的载荷。
///
/// 样本 `fixtures/oauth2-config-parity.json` 钉住「哪些字段被替换」这条规则，两侧读取器
/// 是 `crates/simple-post-core/src/oauth2/config_parity_test.rs` 与
/// `src/lib/__tests__/oauth2-config-parity.test.ts`。
#[test]
fn the_oauth2_payload_is_left_alone_by_the_resolver() {
    let mut env = empty_env();
    env.variables.insert("idp".into(), "idp.example.com".into());
    env.variables.insert("cid".into(), "my-client".into());
    env.variables.insert("secret".into(), "SUPER-SECRET".into());
    env.variables.insert("pw".into(), "hunter2".into());

    let mut ep = minimal_endpoint();
    ep.auth = AuthConfig {
        kind: AuthType::Oauth2,
        oauth2: Some(OAuth2Auth {
            grant_type: OAuth2GrantType::Password,
            token_url: "https://{{idp}}/oauth/token".into(),
            client_id: "{{cid}}".into(),
            client_secret: "{{secret}}".into(),
            password: "{{pw}}".into(),
            scope: "{{idp}}.read".into(),
            username: "u".into(),
            ..OAuth2Auth::default()
        }),
        ..AuthConfig::default()
    };

    let r = resolve_endpoint(&ep, Some(&env), None).unwrap();
    let o = r.auth.oauth2.expect("oauth2 载荷要被透传");
    // 解析阶段一个字段都不替换：替换发生在取 token 之前，由 core 做。
    assert_eq!(o.token_url, "https://{{idp}}/oauth/token");
    assert_eq!(o.client_id, "{{cid}}");
    assert_eq!(o.scope, "{{idp}}.read");
    assert_eq!(o.client_secret, "{{secret}}");
    assert_eq!(o.password, "{{pw}}");
    // 变量池随结果一起走：取 token 时要用的正是这一份。
    assert_eq!(
        r.vars.get("idp").map(String::as_str),
        Some("idp.example.com")
    );
    assert_eq!(
        r.vars.get("secret").map(String::as_str),
        Some("SUPER-SECRET")
    );
    assert!(
        !r.missing_vars.contains(&"idp".to_string()),
        "没参与替换的字段不该报缺变量（它压根没参与解析），got: {:?}",
        r.missing_vars
    );
}

/// `kind` 不是 oauth2 时，那份 oauth2 载荷同样原样留着不动（不解析、不报错）：解析器
/// 压根不碰 oauth2 的载荷，替换由 core 在取 token 之前做。
#[test]
fn an_unselected_oauth2_payload_is_left_alone() {
    let mut env = empty_env();
    env.variables.insert("idp".into(), "idp.example.com".into());
    let mut ep = minimal_endpoint();
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("tok".into()),
        oauth2: Some(OAuth2Auth {
            token_url: "https://{{idp}}/token".into(),
            ..OAuth2Auth::default()
        }),
        ..AuthConfig::default()
    };
    let r = resolve_endpoint(&ep, Some(&env), None).unwrap();
    let o = r.auth.oauth2.expect("载荷仍在");
    assert_eq!(o.token_url, "https://{{idp}}/token");
    assert!(
        !r.missing_vars.contains(&"idp".to_string()),
        "未选中的方案里的变量不算这次请求的问题"
    );
}
