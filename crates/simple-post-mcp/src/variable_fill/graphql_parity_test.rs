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
    #[serde(default)]
    enabled: bool,
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
        6,
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
        let payload = match graphql_payload(Some(&config), &vars, &mut missing, &mut seen) {
            Ok(payload) => payload,
            Err(err) => panic!("用例「{}」组装失败：{err}", case.name),
        };
        let actual: Value = serde_json::from_str(&payload).unwrap_or_else(|e| {
            panic!("用例「{}」的 payload 不是 JSON：{e}\n{payload}", case.name)
        });
        assert_eq!(
            actual, case.expected_payload,
            "用例「{}」的 payload 与样本不一致",
            case.name
        );

        missing.sort();
        assert_eq!(
            missing, case.expected_missing,
            "用例「{}」的缺失变量与样本不一致",
            case.name
        );
    }
}
