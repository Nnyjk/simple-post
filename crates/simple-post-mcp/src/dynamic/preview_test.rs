//! 预览的纯计算部分：响应 → 结果。
//!
//! 这里不起网络，故钉的是「预览能给人什么」这件事：抽取失败仍带正文与 bodyJson、二进制
//! 被点名、截断被透传、处理脚本失败不吞掉抽取结果。

use serde_json::json;

use simple_post_core::domain::{DynamicVariable, Project, ValueMode};

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
