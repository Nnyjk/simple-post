//! 两侧 GraphQL 组装规则的一致性样本：Rust 的 `graphql_payload`（MCP 端）。
//!
//! 同一份样本也被 TS 侧的 `src/lib/graphql.ts::buildGraphQlPayload` 读。两份实现各写一遍
//! 规则（query 与 variables 的 `{{var}}` 替换、variables 的 trim/对象判定、operationName
//! 的空白判定、payload 键序、缺失变量收集），只有一份共享样本才能钉住它们没分叉。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use super::resolve::graphql_payload;
use simple_post_core::domain::GraphQlRequest;

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/graphql-parity.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    #[serde(rename = "$comment")]
    comment: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    graphql: GraphQlFixture,
    #[serde(default)]
    vars: Vec<Var>,
    #[serde(rename = "expectedPayload")]
    expected_payload: Value,
    #[serde(rename = "expectedMissing")]
    expected_missing: Vec<String>,
    /// 组装成功时 `PreparedRequest.body` 的**精确串**；失败为 `null`。
    #[serde(rename = "expectedBody")]
    expected_body: Option<String>,
    /// 失败时问题清单里的一条；成功为 `null`。按子串比较。
    #[serde(rename = "expectedError")]
    expected_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct GraphQlFixture {
    #[serde(default)]
    query: String,
    #[serde(default)]
    variables: String,
    #[serde(default)]
    operation_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Var {
    name: String,
    value: String,
    /// TS 侧只有显式 `false` 才停用；缺省必须是启用，否则同一份样本在两侧含义不同。
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[test]
fn every_graphql_case_builds_the_same_payload_in_rust() {
    let fixture: Fixture =
        serde_json::from_str(FIXTURE).expect("fixtures/graphql-parity.json 读不出来");
    assert!(
        !fixture.comment.trim().is_empty(),
        "样本要保留 $comment，说明它钉的是什么"
    );
    assert_eq!(
        fixture.cases.len(),
        10,
        "用例条数变了：键名漂移会让这条测试静默空转"
    );

    for case in &fixture.cases {
        let mut vars = HashMap::new();
        for var in case.vars.iter().filter(|var| var.enabled) {
            vars.insert(var.name.clone(), var.value.clone());
        }
        let config = GraphQlRequest {
            query: case.graphql.query.clone(),
            variables: case.graphql.variables.clone(),
            operation_name: case.graphql.operation_name.clone(),
        };
        let mut missing = Vec::new();
        let mut seen = HashMap::new();
        match graphql_payload(Some(&config), &vars, &mut missing, &mut seen) {
            Ok(payload) => {
                assert!(
                    case.expected_error.is_none(),
                    "用例「{}」应当失败（{}），实际却组装成功：{payload}",
                    case.name,
                    case.expected_error.as_deref().unwrap_or_default()
                );
                assert_eq!(
                    Some(&payload),
                    case.expected_body.as_ref(),
                    "用例「{}」的正文必须逐字节相同（键序也算）",
                    case.name
                );
                let actual: Value = serde_json::from_str(&payload).unwrap_or_else(|e| {
                    panic!("用例「{}」的 payload 不是 JSON：{e}\n{payload}", case.name)
                });
                assert_eq!(
                    actual, case.expected_payload,
                    "用例「{}」的 payload 与样本不一致",
                    case.name
                );
            }
            Err(err) => {
                let message = format!("{err}");
                let expected = case.expected_error.as_deref().unwrap_or_else(|| {
                    panic!("用例「{}」应当成功，实际失败：{message}", case.name)
                });
                assert!(
                    message.contains(expected),
                    "用例「{}」的错误与样本不一致：{message}",
                    case.name
                );
                assert!(
                    case.expected_body.is_none(),
                    "用例「{}」失败时不该有期望正文",
                    case.name
                );
                assert!(
                    case.expected_payload.is_null(),
                    "用例「{}」失败时 expectedPayload 必须是 null",
                    case.name
                );
            }
        }

        // 两侧都排：只排 actual 会让「样本自己没排」永远抓不到。
        let mut expected_missing = case.expected_missing.clone();
        expected_missing.sort();
        missing.sort();
        assert_eq!(
            missing, expected_missing,
            "用例「{}」的缺失变量与样本不一致",
            case.name
        );
    }
}
