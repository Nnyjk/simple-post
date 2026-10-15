//! 鉴权注入与 oauth2 载荷的选择性替换。

use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, KeyValue, OAuth2Auth,
    OAuth2GrantType,
};

use super::resolve::resolve_endpoint;
use super::test_support::{
    empty_env, in_module_a, minimal_endpoint, project_with_defs, set_cell, MOD_A,
};
// ---- auth injection ----------------------------------------------

#[test]
fn resolve_endpoint_bearer_auth_injects_authorization_header() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "secret-xyz".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{token}}".into()),
        basic: None,
        apikey: None,
        oauth2: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
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
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("u".into(), "alice".into());
    env.variables.insert("p".into(), "wonder".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
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
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
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
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "ABC123".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
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
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
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
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "Q-VAL".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
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
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
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
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "from-env".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
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
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // The user's "Bearer user-set" should win.
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .unwrap();
    assert_eq!(auth.1, "Bearer user-set");
}

// ---- oauth2 ------------------------------------------------------

/// OAuth2 配置里 `{{var}}` 的替换是**有选择的**：地址与标识替换，密钥不替换。
///
/// 密钥那半边不是省事。`resolve_auth` 的返回值进 `ResolvedEndpoint.auth`，而
/// `api_resolve_endpoint` 会把整个 auth 回显给 agent —— 一旦 `clientSecret` 也被解出来，
/// 环境变量表里那份 secret 就会被复制进一次工具响应。
#[test]
fn oauth2_substitutes_addresses_and_identifiers_but_not_secrets() {
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

    let r = resolve_endpoint(&ep, Some(&env), &project_with_defs(), None).unwrap();
    let o = r.auth.oauth2.expect("oauth2 载荷要被透传");
    assert_eq!(o.token_url, "https://idp.example.com/oauth/token");
    assert_eq!(o.client_id, "my-client");
    assert_eq!(o.scope, "idp.example.com.read");
    assert_eq!(o.client_secret, "{{secret}}", "密钥不参与替换");
    assert_eq!(o.password, "{{pw}}", "密码不参与替换");
    assert!(
        !r.missing_vars.contains(&"secret".to_string()),
        "没被解析的字段不该报缺变量（它压根没参与解析），got: {:?}",
        r.missing_vars
    );
}

/// `kind` 不是 oauth2 时，那份 oauth2 载荷原样留着不动（不解析、不报错），与
/// bearer/basic 的处理一致：只处理选中的那一种。
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
    let r = resolve_endpoint(&ep, Some(&env), &project_with_defs(), None).unwrap();
    let o = r.auth.oauth2.expect("载荷仍在");
    assert_eq!(o.token_url, "https://{{idp}}/token");
    assert!(
        !r.missing_vars.contains(&"idp".to_string()),
        "未选中的方案里的变量不算这次请求的问题"
    );
}
