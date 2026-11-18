//! 预览的纯计算部分：响应 → 结果。
//!
//! 这里不起网络，故钉的是「预览能给人什么」这件事：抽取失败仍带正文与 bodyJson、二进制
//! 被点名、截断被透传、处理脚本失败不吞掉抽取结果。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use simple_post_core::db;
use simple_post_core::domain::{
    AuthConfig, DynamicValueSource, DynamicVariable, EndpointDetail, Environment, HttpMethod,
    Project, RequestBody, ResourceNode, ResourceNodeType, ValueMode,
};

use super::*;

/// 一份草稿摊成的定义。预览只用到名字、抽取规则与处理脚本。
fn definition(extract_path: &str, transform_script: Option<&str>) -> DynamicVariable {
    DynamicVariable {
        id: String::new(),
        project_id: None,
        name: "sns".to_string(),
        description: None,
        source_endpoint_id: Some("ep1".to_string()),
        source_endpoint_name: None,
        source_inline_json: None,
        source_environment_id: None,
        extract_path: extract_path.to_string(),
        transform_script: transform_script.map(str::to_string),
        value_mode: ValueMode::Auto,
        ttl_seconds: 0,
        stale_fallback: false,
        enabled: true,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    }
}

fn raw(body: &str) -> RawSource {
    RawSource {
        status: 200,
        content_type: "application/json".to_string(),
        body: body.to_string(),
        truncated: false,
        body_base64: None,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        duration_ms: 12,
    }
}

/// 内联请求的取数上下文：没有项目、没有环境。
fn context() -> SourceContext {
    SourceContext {
        project_id: None,
        environment_id: None,
        env: None,
        project: Project::empty(),
        module_id: None,
    }
}

#[tokio::test]
async fn a_missed_path_still_returns_the_body_and_its_json() {
    // 用户点完「预览」看不见树，就没法改 JSONPath —— 所以没命中也必须把正文原样带回。
    let def = definition("$.nope", None);
    let out = evaluate(&def, &raw(r#"{"a":{"b":7}}"#), &context()).await;

    assert_eq!(out.body_json, Some(json!({"a": {"b": 7}})));
    assert_eq!(out.extracted, None);
    assert_eq!(out.value, None);
    let issue = out.extract_error.expect("没命中要有错误");
    assert_eq!(issue.kind, "no_match");
    assert!(issue.message.contains("$.nope"), "{}", issue.message);
    assert!(!out.binary);
    assert!(!out.truncated);
}

#[tokio::test]
async fn a_non_json_body_is_reported_as_not_json_but_kept_as_text() {
    // HTML 错误页不能被降级成一个字符串节点 —— 那会让用户以为接口返回了 JSON。
    let def = definition("$", None);
    let out = evaluate(&def, &raw("<html>503</html>"), &context()).await;

    assert_eq!(out.body_json, None);
    assert_eq!(out.body_text, "<html>503</html>");
    assert_eq!(out.extract_error.expect("要有错误").kind, "not_json");
}

#[tokio::test]
async fn a_binary_body_says_so_instead_of_showing_an_empty_tree() {
    let mut binary = raw("");
    binary.body_base64 = Some("AAEC".to_string());
    binary.content_type = "image/png".to_string();

    let def = definition("$", None);
    let out = evaluate(&def, &binary, &context()).await;

    assert!(out.binary);
    assert_eq!(out.body_json, None, "二进制正文不解析");
    assert_eq!(out.body_text, "", "正文不是合法 UTF-8");
    let issue = out.extract_error.expect("要说清楚为什么没有树");
    assert_eq!(issue.kind, "binary");
    assert!(issue.message.contains("二进制"), "{}", issue.message);
}

#[tokio::test]
async fn a_truncated_body_is_surfaced() {
    let mut truncated = raw(r#"{"a":"#);
    truncated.truncated = true;

    let out = evaluate(&definition("$", None), &truncated, &context()).await;
    assert!(out.truncated);
}

#[tokio::test]
async fn a_blank_path_extracts_the_whole_response() {
    // 空串与全空白都回落到 `$`，与 `core::dynamic::extract_value` 同一套规则。
    let out = evaluate(&definition("   ", None), &raw(r#"{"a":1}"#), &context()).await;

    assert_eq!(out.extract_path, "$");
    assert_eq!(out.extracted, Some(json!({"a": 1})));
    assert!(out.extract_error.is_none());
}

#[tokio::test]
async fn a_working_transform_script_becomes_the_value() {
    let def = definition("$.a", Some("return pm.data + 1;"));
    let out = evaluate(&def, &raw(r#"{"a":41}"#), &context()).await;

    assert_eq!(out.extracted, Some(json!(41)), "抽取结果要留着");
    assert_eq!(out.value, Some(json!(42)));
    assert!(out.transform_error.is_none());
}

#[tokio::test]
async fn a_broken_transform_script_keeps_the_extracted_value() {
    // 真实链路里脚本失败按「缺失」处理；预览不能因此把抽取结果也藏起来 ——
    // 用户此刻要判断的是 JSONPath 对不对。
    let def = definition("$.a", Some("throw new Error('炸了');"));
    let out = evaluate(&def, &raw(r#"{"a":41}"#), &context()).await;

    assert_eq!(out.extracted, Some(json!(41)));
    assert_eq!(
        out.value,
        Some(json!(41)),
        "脚本失败时 value 回落到抽取结果"
    );
    let issue = out.transform_error.expect("脚本失败要报出来");
    assert_eq!(issue.kind, "script_error");
}

#[test]
fn blank_draft_fields_mean_absent() {
    // 界面把没选的字段发成空串。若原样交给取数器，「引用端点」形态会去查一个 id 为空串的
    // 端点，报「端点已被删除」而不是「没有数据源」。
    let draft = PreviewDraft {
        source_endpoint_id: Some("   ".to_string()),
        source_inline_json: Some(String::new()),
        source_environment_id: Some("".to_string()),
        extract_path: None,
        ..PreviewDraft::default()
    };
    let def = draft.into_definition();
    assert_eq!(def.source_endpoint_id, None);
    assert_eq!(def.source_inline_json, None);
    assert_eq!(def.source_environment_id, None);
    assert_eq!(def.extract_path, "$");
}

#[test]
fn endpoint_wins_when_both_sources_are_given() {
    // 与真实取数的判定一致：两个都填时以引用端点为先。
    let draft = PreviewDraft {
        source_endpoint_id: Some("ep1".to_string()),
        source_inline_json: Some(r#"{"url":"https://example.test"}"#.to_string()),
        ..PreviewDraft::default()
    };
    assert_eq!(
        draft.into_definition().source_endpoint_id.as_deref(),
        Some("ep1")
    );
}

#[tokio::test]
async fn a_non_2xx_response_keeps_the_body_but_has_no_value() {
    // 401 的 `{"token":"expired"}` 不是值：运行时按缺失处理，预览不能把它当最终值展示。
    let mut rejected = raw(r#"{"token":"expired"}"#);
    rejected.status = 401;

    let out = evaluate(&definition("$.token", None), &rejected, &context()).await;

    assert_eq!(out.body_json, Some(json!({"token": "expired"})));
    assert_eq!(out.extracted, Some(json!("expired")), "抽取照做");
    assert_eq!(out.value, None, "非 2xx 不取值");
    let issue = out.extract_error.expect("要说清楚为什么没取值");
    assert_eq!(issue.kind, "http_status");
    assert!(issue.message.contains("401"), "{}", issue.message);
}

#[tokio::test]
async fn an_oversized_value_is_reported_as_too_large() {
    // 小 200 响应 + 一个产出 300 KiB 字符串的脚本：线上按缺失处理，预览也不能显示成最终值。
    let def = definition("$.a", Some("return 'x'.repeat(300 * 1024);"));
    let out = evaluate(&def, &raw(r#"{"a":1}"#), &context()).await;

    assert_eq!(out.extracted, Some(json!(1)), "抽取结果要留着");
    assert!(out.value.is_none(), "超限不取值");
    let issue = out.extract_error.expect("超限要有错误");
    assert_eq!(issue.kind, "too_large");
    assert!(issue.message.contains("262144"), "{}", issue.message);
}

/// 一个项目 `p1`（含模块与取数端点）加另一个项目 `p2` 的环境。
fn pool_with_a_foreign_environment() -> DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-preview-test-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = simple_post_core::db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        for (id, name) in [("p1", "First"), ("p2", "Second")] {
            db::projects::create(
                c,
                &Project {
                    id: id.into(),
                    name: name.into(),
                    description: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )?;
        }
        db::nodes::create(
            c,
            &ResourceNode {
                id: "m1".into(),
                root_id: "p1".into(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "模块".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            None,
        )?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e1".into(),
                root_id: "p1".into(),
                parent_id: Some("m1".into()),
                kind: ResourceNodeType::Endpoint,
                name: "取数接口".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            Some(&EndpointDetail {
                node_id: "e1".into(),
                method: HttpMethod::Get,
                url: "/points".into(),
                base_url_override: None,
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
                tls: None,
                responses: vec![],
                protocol: Default::default(),
                graphql: None,
            }),
        )?;
        db::environments::create(
            c,
            &Environment {
                id: "env_p2".into(),
                project_id: "p2".into(),
                name: "other".into(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .expect("seed");
    pool
}

/// 一条被叫到就算失败的取数器：环境判定必须发生在发请求之前。
struct NeverFetcher {
    calls: AtomicUsize,
}

impl SourceFetcher for NeverFetcher {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        _ctx: &'a ResolveContext,
        _source: &'a SourceContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(DynamicError {
                name: def.name.clone(),
                kind: DynamicErrorKind::Network,
                message: "不该被调用".into(),
            })
        })
    }
}

/// 定义点名的环境不属于数据源接口所属的项目：预览与解析给同一个结论。
#[tokio::test]
async fn a_named_environment_of_another_project_is_refused_by_preview_and_resolve() {
    let pool = pool_with_a_foreign_environment();
    let draft = PreviewDraft {
        name: Some("sns".into()),
        source_endpoint_id: Some("e1".into()),
        source_environment_id: Some("env_p2".into()),
        ..PreviewDraft::default()
    };

    // 预览入口：整条预览失败，文案与保存侧那句对齐。
    let err = preview(&pool, draft.clone(), None)
        .await
        .expect_err("预览要拒");
    assert!(
        matches!(err, AppError::Validation(_)),
        "要按校验错误报出来：{err:?}"
    );
    assert!(
        err.to_string().contains("不在数据源接口所属的项目里"),
        "{err}"
    );

    // 解析入口：同一个结论，按 source_error 报出来并当缺失处理。
    let definition = draft.into_definition();
    let ctx = ResolveContext {
        pool: pool.clone(),
        environment_id: None,
    };
    let fetcher = NeverFetcher {
        calls: AtomicUsize::new(0),
    };
    let status = simple_post_core::dynamic::resolve_definition(&ctx, &definition, true, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0, "判定在发请求之前");
    assert_eq!(status.source, DynamicValueSource::Missing);
    assert!(status.value.is_none());
    let error = status.error.expect("要报取数失败");
    assert_eq!(error.kind, DynamicErrorKind::SourceError);
    assert!(
        error.message.contains("不在数据源接口所属的项目里"),
        "{}",
        error.message
    );
}
