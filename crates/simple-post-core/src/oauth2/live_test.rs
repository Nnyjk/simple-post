// 真的发一次 HTTP：form 体、`Authorization: Basic`、刷新与错误应答在真实 socket 上成立。

use super::store::unix_secs;
use super::test_fixtures::{client_credentials, fresh_pool, kv, FakeIdp};
use super::*;
use crate::db;
use crate::domain::{OAuth2Auth, OAuth2GrantType};

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
