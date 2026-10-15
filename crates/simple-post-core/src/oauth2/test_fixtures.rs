//! 仅测试：oauth2 各测试文件共用的配置助手与假 IdP。

use std::path::PathBuf;

use crate::db::DbPool;
use crate::domain::{KeyValue, OAuth2Auth, OAuth2GrantType};

pub(crate) fn fresh_pool() -> DbPool {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "simple-post-oauth2-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    crate::db::init(&dir).expect("init")
}

pub(crate) fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        id: format!("kv_{key}"),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }
}

/// 一份能过 `validate` 的 `client_credentials` 配置。
pub(crate) fn client_credentials() -> OAuth2Auth {
    OAuth2Auth {
        grant_type: OAuth2GrantType::ClientCredentials,
        token_url: "https://idp.example.com/oauth/token".into(),
        client_id: "cid".into(),
        client_secret: "csecret".into(),
        scope: "read".into(),
        ..OAuth2Auth::default()
    }
}

/// 一个只说「收下这一行」的假 IdP：把收到的请求头 + body 交回调用方，
/// 然后回一段固定响应。用来验证 form 体、`Authorization: Basic` 与响应解析
/// 在真实 socket 上成立——静态断言看不出 URL 拼错或 header 没发出去。
pub(crate) struct FakeIdp {
    pub url: String,
    handle: tokio::task::JoinHandle<String>,
}

impl FakeIdp {
    pub async fn start(
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

    pub async fn received(self) -> String {
        self.handle.await.unwrap()
    }
}
