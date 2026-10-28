//! 执行链的单元测试：变量回写失败的后果、前置阶段的写入对测试阶段的可见性。
//!
//! 这里要一个真的 loopback 服务端：脚本改写后的请求必须真的发出去，故「发出去的
//! 是不是改写后的那个」只能对着真实的上游断言。

use std::collections::BTreeMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::db;
use crate::domain::{EnvSnapshot, RequestSettings, TlsSettings};

/// 回固定响应的假上游，并把收到的请求行记下。
struct Stub {
    url: String,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    _handle: tokio::task::JoinHandle<()>,
}

async fn stub() -> Stub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let handle = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8 * 1024];
            let Ok(n) = sock.read(&mut buf).await else {
                continue;
            };
            sink.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let body = r#"{"id":7}"#;
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
        _handle: handle,
    }
}

fn temp_pool(tag: &str) -> db::DbPool {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-script-send-test-{}-{}-{id}-{tag}",
        std::process::id(),
        crate::domain::now_ms(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(&dir).expect("db")
}

fn settings() -> RequestSettings {
    RequestSettings {
        timeout_ms: 5_000,
        follow_redirects: true,
        max_response_size_kb: 1_024,
        tls: TlsSettings::default(),
    }
}

fn request(url: &str) -> ScriptRequest {
    ScriptRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![],
        body: None,
    }
}

#[tokio::test]
async fn a_failed_environment_write_back_does_not_fail_the_send() {
    let pool = temp_pool("write-back");
    let upstream = stub().await;

    let outcome = send_with_scripts(
        &pool,
        ScriptedSendInput {
            endpoint_id: None,
            request: request(&format!("{}/x", upstream.url)),
            // 这个环境不存在 → 回写必然失败。
            env: EnvSnapshot {
                id: Some("env-does-not-exist".to_string()),
                variables: BTreeMap::new(),
            },
            settings: settings(),
            tls_override: None,
            // 脚本确实写了变量，才会走到回写那一步。
            script_override: Some(ScriptOverride {
                pre_request: Some("pm.environment.set('token', 'v');".to_string()),
                test: None,
            }),
        },
    )
    .await
    .expect("发送本身不该失败");

    // 请求的成败与脚本的产出都还在。
    assert!(
        outcome.http.response.is_some(),
        "响应不能因为回写失败而丢掉"
    );
    let script = outcome.script.expect("有脚本就有产出");
    assert!(
        script
            .errors
            .iter()
            .any(|e| e.message.contains("environment writes")),
        "回写失败要被记成一条脚本错误：{:?}",
        script.errors
    );
    assert!(
        script.env_writes.is_empty(),
        "没落库的写入不该报成「已应用」"
    );
}

#[tokio::test]
async fn the_pre_request_writes_are_visible_to_the_test_phase() {
    let pool = temp_pool("pre-to-test");
    let upstream = stub().await;

    let outcome = send_with_scripts(
        &pool,
        ScriptedSendInput {
            endpoint_id: None,
            request: request(&format!("{}/x", upstream.url)),
            env: EnvSnapshot {
                id: None,
                variables: BTreeMap::new(),
            },
            settings: settings(),
            tls_override: None,
            script_override: Some(ScriptOverride {
                pre_request: Some("pm.environment.set('token', 'from-pre');".to_string()),
                test: Some(
                    "pm.test('看到前置写入的变量', function () { \
                       pm.expect(String(pm.environment.get('token'))).to.equal('from-pre'); \
                     });"
                    .to_string(),
                ),
            }),
        },
    )
    .await
    .expect("发送");

    let script = outcome.script.expect("有脚本就有产出");
    let assertions = &script.assertions;
    assert_eq!(assertions.len(), 1, "{:?}", script.errors);
    assert!(
        assertions[0].passed,
        "测试脚本要看得见前置脚本写下的变量：{:?}",
        assertions[0]
    );
}

#[tokio::test]
async fn the_scripts_rewrite_the_request_that_is_actually_sent() {
    let pool = temp_pool("rewrite");
    let upstream = stub().await;

    let outcome = send_with_scripts(
        &pool,
        ScriptedSendInput {
            endpoint_id: None,
            request: request(&format!("{}/x", upstream.url)),
            env: EnvSnapshot::default(),
            settings: settings(),
            tls_override: None,
            script_override: Some(ScriptOverride {
                pre_request: Some("pm.request.url = pm.request.url + '/rewritten';".to_string()),
                test: None,
            }),
        },
    )
    .await
    .expect("发送");

    let requests = upstream.seen.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /x/rewritten HTTP/1.1"),
        "发出去的必须是脚本改写后的那个：{}",
        requests[0]
    );
    assert_eq!(outcome.request.url, format!("{}/x/rewritten", upstream.url));
}
