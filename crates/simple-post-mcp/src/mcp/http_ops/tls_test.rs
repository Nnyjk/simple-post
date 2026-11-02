//! MCP 侧的 HTTPS 出站设置：临时覆盖怎么解析、端点自带的那份与临时覆盖谁赢。
//!
//! 这一组守三件事：
//!
//! 1. `settings_override.tls` 用的是 **snake_case** 键，而 `TlsSettings` 自己的 serde
//!    键名是 camelCase —— 直接反序列化会**静默**把每一项都填成默认值，故必须有一份
//!    显式的转换。
//! 2. 优先级：本次调用的临时覆盖压过端点自带的那一份。
//! 3. 合成结果真的落到了发请求的那一步 —— 断言的是执行器报出的那个证书路径，
//!    而不是一个中间变量。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};

use super::input::{parse_tls_settings, resolve_tls_override};
use super::tool_api_call_endpoint;
use crate::mcp::AppState;
use simple_post_core::db;
use simple_post_core::domain::{
    AuthConfig, EndpointDetail, HttpMethod, Permission, Project, RequestBody, ResourceNode,
    ResourceNodeType, TlsSettings,
};

/// 每个用例一份干净的库。
fn mcp_state(tag: &str) -> Arc<AppState> {
    static N: AtomicU32 = AtomicU32::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-tls-{}-{tag}-{id}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// 种一个项目与一个端点，端点自带一份 HTTPS 设置。
///
/// base URL 指向一个没人监听的端口：这条用例断言的是**发请求之前**就失败的那一步
/// （证书文件读不到），请求根本不会出去。
fn seed(st: &AppState, endpoint_tls: Option<TlsSettings>) -> String {
    let project = Project {
        id: "p_tls".into(),
        name: "P".into(),
        description: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let module = ResourceNode {
        id: "m_tls".into(),
        root_id: "p_tls".into(),
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: "m".into(),
        description: None,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    };
    let detail = EndpointDetail {
        node_id: "e_tls".into(),
        method: HttpMethod::Get,
        url: "/x".into(),
        base_url_override: Some("http://127.0.0.1:1".into()),
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: endpoint_tls,
    };
    db::with_tx(&st.pool, |c| {
        db::projects::create(c, &project)?;
        db::permissions::set(c, "p_tls", Permission::Full)?;
        db::nodes::create(c, &module, None)?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e_tls".into(),
                root_id: "p_tls".into(),
                parent_id: Some("m_tls".into()),
                kind: ResourceNodeType::Endpoint,
                name: "端点".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            Some(&detail),
        )?;
        Ok(())
    })
    .expect("seed");
    "e_tls".into()
}

/// 一份只带 CA 路径的设置。路径故意指向一个不存在的文件：执行器读不到它时会
/// 把**路径本身**报出来，故路径就成了「这次请求用了哪一份设置」的证据。
fn tls_with_ca(path: &str) -> TlsSettings {
    TlsSettings {
        skip_verify: false,
        ca_certificate_path: Some(path.to_string()),
        client_certificate_path: None,
        client_key_path: None,
    }
}

// =========================================================================
// 1. `settings_override.tls` 的键名是 snake_case
// =========================================================================

/// `settings_override` 里既有的三个键都是 snake_case，`tls` 内部跟着用 snake_case。
#[test]
fn settings_override_tls_is_read_with_snake_case_keys() {
    let v = json!({
        "skip_verify": true,
        "ca_certificate_path": "/etc/ca.pem",
        "client_certificate_path": "/etc/client.pem",
        "client_key_path": "/etc/client.key",
    });
    let tls = parse_tls_settings(Some(&v))
        .expect("形状对的载荷必须被接受")
        .expect("给了载荷就有一份设置");

    assert!(tls.skip_verify, "skip_verify 必须被读到");
    assert_eq!(tls.ca_certificate_path.as_deref(), Some("/etc/ca.pem"));
    assert_eq!(
        tls.client_certificate_path.as_deref(),
        Some("/etc/client.pem")
    );
    assert_eq!(tls.client_key_path.as_deref(), Some("/etc/client.key"));
}

/// **camelCase 的拼法在这里不生效** —— 这正是要一份显式转换函数的原因：
/// `TlsSettings` 的每个字段都带默认值，直接反序列化时键名对不上也不报错，
/// 于是「跳过校验」与「挂上客户端证书」会静默地全部失效。
#[test]
fn the_camel_case_spelling_is_not_what_settings_override_reads() {
    let v = json!({
        "skipVerify": true,
        "caCertificatePath": "/etc/ca.pem",
        "clientCertificatePath": "/etc/client.pem",
        "clientKeyPath": "/etc/client.key",
    });
    let tls = parse_tls_settings(Some(&v))
        .expect("形状对的载荷必须被接受")
        .expect("给了载荷就有一份设置");

    assert!(
        !tls.skip_verify,
        "camelCase 的键名不是这一层的契约：`skip_verify` 才是"
    );
    assert_eq!(tls.ca_certificate_path, None);
    assert_eq!(tls.client_certificate_path, None);
    assert_eq!(tls.client_key_path, None);
}

/// `null` 与缺席都取默认值：没有路径就是没有路径，`skip_verify` 没给就是 `false`。
#[test]
fn null_values_and_missing_fields_fall_back_to_the_defaults() {
    let v = json!({
        "skip_verify": null,
        "ca_certificate_path": null,
    });
    let tls = parse_tls_settings(Some(&v))
        .expect("null 不是错误")
        .expect("给了载荷就有一份设置");

    assert!(!tls.skip_verify, "null 的 skip_verify 取默认值 false");
    assert_eq!(tls.ca_certificate_path, None);
    assert_eq!(tls.client_certificate_path, None, "缺席的字段取默认值");
    assert_eq!(tls.client_key_path, None);

    // 一个空对象同样是一份「什么都不改」的有效设置。
    let empty = parse_tls_settings(Some(&json!({})))
        .expect("空对象不是错误")
        .expect("给了载荷就有一份设置");
    assert_eq!(empty, TlsSettings::default());
}

/// 没有 `tls` 键 = 没有覆盖：这一层不替全局默认做任何决定。
#[test]
fn an_absent_tls_key_means_no_override() {
    assert_eq!(parse_tls_settings(None).expect("缺席不是错误"), None);
    assert_eq!(
        parse_tls_settings(Some(&Value::Null)).expect("null 不是错误"),
        None
    );
    assert_eq!(
        resolve_tls_override(Some(&json!({})), None).expect("没有 tls 键"),
        None,
        "`settings_override` 里没有 `tls` 键时不产生任何覆盖"
    );
}

/// 值的类型不对要报错并点名那个键：路径写错会让请求在**没有**客户端证书的情况下
/// 发出去，而调用方不会知道自己传的路径没被收下。
#[test]
fn a_wrong_typed_path_is_reported_instead_of_being_dropped() {
    let err = parse_tls_settings(Some(&json!({ "client_certificate_path": 42 })))
        .expect_err("类型不对必须报错");
    let text = err.to_string();
    assert!(
        text.contains("client_certificate_path"),
        "错误必须点名那个键：{text}"
    );

    let err =
        parse_tls_settings(Some(&json!({ "skip_verify": "yes" }))).expect_err("类型不对必须报错");
    let text = err.to_string();
    assert!(text.contains("skip_verify"), "错误必须点名那个键：{text}");

    let err = parse_tls_settings(Some(&json!("nope"))).expect_err("类型不对必须报错");
    let text = err.to_string();
    assert!(text.contains("tls"), "错误必须说清是 `tls` 的问题：{text}");
}

// =========================================================================
// 2. 端点自带的那份与本次调用的临时覆盖，谁赢
// =========================================================================

/// 优先级：本次调用的临时覆盖 > 端点自带的那份 > 全局默认（缺席时返回 `None`，
/// 由 `send_with_scripts` 去取全局默认）。
#[test]
fn the_call_time_override_beats_the_endpoints_own_settings() {
    let endpoint_tls = tls_with_ca("/endpoint-ca.pem");
    let override_payload = json!({ "tls": { "ca_certificate_path": "/override-ca.pem" } });

    // 端点有值、覆盖也有值 → 覆盖赢。
    let won = resolve_tls_override(Some(&override_payload), Some(endpoint_tls.clone()))
        .expect("形状对的载荷")
        .expect("有一份设置");
    assert_eq!(
        won.ca_certificate_path.as_deref(),
        Some("/override-ca.pem"),
        "本次调用的临时覆盖必须压过端点自带的那份"
    );

    // 端点有值、覆盖没有 → 端点自带的那份生效。
    let kept = resolve_tls_override(None, Some(endpoint_tls.clone()))
        .expect("没有覆盖")
        .expect("端点自带的那份");
    assert_eq!(
        kept.ca_certificate_path.as_deref(),
        Some("/endpoint-ca.pem"),
        "没有临时覆盖时端点自带的那份生效"
    );

    // 端点没有、覆盖有 → 覆盖生效。
    let only_override = resolve_tls_override(Some(&override_payload), None)
        .expect("形状对的载荷")
        .expect("有一份设置");
    assert_eq!(
        only_override.ca_certificate_path.as_deref(),
        Some("/override-ca.pem")
    );

    // 两档都没有 → 继承全局默认。
    assert_eq!(
        resolve_tls_override(None, None).expect("没有覆盖"),
        None,
        "两档都缺席时交给全局默认"
    );
}

// =========================================================================
// 3. 合成结果真的落到了发请求的那一步
// =========================================================================

/// 端点自带的那份生效时，报错里出现的是**端点那份**的路径。
#[tokio::test]
async fn the_endpoints_own_settings_reach_the_sender() {
    let st = mcp_state("endpoint-own");
    let endpoint_id = seed(&st, Some(tls_with_ca("/no/such/endpoint-ca.pem")));

    let result = tool_api_call_endpoint(&st, json!({ "endpoint_id": endpoint_id }))
        .await
        .expect("调用本身不失败");
    assert!(result.is_error, "证书读不到必须是一次失败");
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(
        text.contains("/no/such/endpoint-ca.pem"),
        "端点自带的证书路径必须真的被用上：{text}"
    );
}

/// 本次调用的临时覆盖压过端点自带的那一份：报出来的是**覆盖那份**的路径。
#[tokio::test]
async fn the_call_time_override_reaches_the_sender_instead_of_the_endpoints_own() {
    let st = mcp_state("override-wins");
    let endpoint_id = seed(&st, Some(tls_with_ca("/no/such/endpoint-ca.pem")));

    let result = tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": endpoint_id,
            "settings_override": {
                "tls": { "ca_certificate_path": "/no/such/override-ca.pem" }
            }
        }),
    )
    .await
    .expect("调用本身不失败");
    assert!(result.is_error, "证书读不到必须是一次失败");
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(
        text.contains("/no/such/override-ca.pem"),
        "本次调用的临时覆盖必须压过端点自带的那份：{text}"
    );
    assert!(
        !text.contains("/no/such/endpoint-ca.pem"),
        "端点自带的那份被压过，不该再被读到：{text}"
    );
}

/// 端点没有自带设置时，本次调用的临时覆盖仍然生效。
#[tokio::test]
async fn the_call_time_override_reaches_the_sender_without_an_endpoint_setting() {
    let st = mcp_state("override-only");
    let endpoint_id = seed(&st, None);

    let result = tool_api_call_endpoint(
        &st,
        json!({
            "endpoint_id": endpoint_id,
            "settings_override": {
                "tls": { "ca_certificate_path": "/no/such/override-ca.pem" }
            }
        }),
    )
    .await
    .expect("调用本身不失败");
    assert!(result.is_error, "证书读不到必须是一次失败");
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(
        text.contains("/no/such/override-ca.pem"),
        "临时覆盖必须在没有端点设置时也生效：{text}"
    );
}
