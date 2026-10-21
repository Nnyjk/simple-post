// 线格式 / 入参：两条传输层共同入口 `resolve`、重定向与单飞闸。

use super::store::unix_secs;
use super::test_fixtures::{client_credentials, fresh_pool, FakeIdp};
use super::*;
use crate::db;
use crate::domain::{AuthConfig, AuthType, OAuth2Auth, OAuth2GrantType};

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
/// 轮换 refresh token 的 IdP 会让第二次失败，而我们「刷新失败就整取」的回退路径会拿已消费
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
