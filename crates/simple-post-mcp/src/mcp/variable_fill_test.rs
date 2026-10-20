use super::*;
use crate::mcp::arg_str_opt;
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode, HttpMethod, KeyValue,
    RequestBody,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-var-fill-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(crate::mcp::AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

fn ep(url: &str, auth: AuthConfig, body_mode: BodyMode, body_content: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: "ep1".into(),
        method: HttpMethod::Get,
        url: url.into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: Vec::<KeyValue>::new(),
        headers: Vec::<KeyValue>::new(),
        body: RequestBody {
            mode: body_mode,
            content: body_content.into(),
            schema: None,
        },
        auth,
        docs: None,
        notes: None,
        tags: Vec::new(),
        pre_request_script: None,
        test_script: None,
    }
}

#[test]
fn table_has_three_rows() {
    assert_eq!(table().len(), 3);
}

#[tokio::test]
async fn resolve_template_rejects_empty() {
    let res = tool_api_resolve_template(&state(), json!({ "template": "" })).await;
    assert!(matches!(res, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn resolve_template_returns_resolved_and_missing() {
    let res = tool_api_resolve_template(
        &state(),
        json!({ "template": "https://{{host}}/users/{{id}}" }),
    )
    .await
    .unwrap();
    let text = serde_json::to_string(&res).unwrap();
    assert!(text.contains("missing"));
    assert!(text.contains("host"));
    assert!(text.contains("id"));
}

#[test]
fn validate_endpoint_passes_for_well_formed_endpoint() {
    let endpoint = ep(
        "https://api.example.com/users",
        AuthConfig::default(),
        BodyMode::None,
        "",
    );
    let issues = validate(&endpoint);
    assert!(issues.is_empty(), "expected no issues, got {issues:?}");
}

#[test]
fn validate_endpoint_flags_empty_url() {
    let endpoint = ep("", AuthConfig::default(), BodyMode::None, "");
    assert!(!validate(&endpoint).is_empty());
}

#[test]
fn validate_endpoint_flags_missing_bearer() {
    let auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some(String::new()),
        ..Default::default()
    };
    let endpoint = ep("/x", auth, BodyMode::None, "");
    let issues = validate(&endpoint);
    assert!(issues.iter().any(|i| i["field"] == "auth.bearer"));
}

#[test]
fn validate_endpoint_flags_missing_basic() {
    let auth = AuthConfig {
        kind: AuthType::Basic,
        basic: Some(BasicAuth {
            username: String::new(),
            password: String::new(),
        }),
        ..Default::default()
    };
    let endpoint = ep("/x", auth, BodyMode::None, "");
    let issues = validate(&endpoint);
    assert!(issues.iter().any(|i| i["field"] == "auth.basic.username"));
    assert!(issues.iter().any(|i| i["field"] == "auth.basic.password"));
}

#[test]
fn validate_endpoint_flags_missing_apikey() {
    let auth = AuthConfig {
        kind: AuthType::Apikey,
        apikey: Some(ApiKeyAuth {
            key: String::new(),
            value: String::new(),
            in_location: ApiKeyLocation::Header,
        }),
        ..Default::default()
    };
    let endpoint = ep("/x", auth, BodyMode::None, "");
    let issues = validate(&endpoint);
    assert!(issues.iter().any(|i| i["field"] == "auth.apikey.key"));
    assert!(issues.iter().any(|i| i["field"] == "auth.apikey.value"));
}

#[test]
fn validate_endpoint_flags_empty_body_when_required() {
    let endpoint = ep("/x", AuthConfig::default(), BodyMode::Json, "");
    let issues = validate(&endpoint);
    assert!(issues.iter().any(|i| i["field"] == "body.content"));
}

#[test]
fn arg_str_opt_returns_none_for_missing() {
    let v = json!({});
    assert_eq!(arg_str_opt(&v, "x"), None);
}
