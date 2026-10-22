//! multipart 装配的一致性样本：Rust 的 `resolve_endpoint` + `multipart::apply`（MCP 端）。
//!
//! 同一份样本也被 TS 侧的
//! `src/lib/__tests__/http-build-multipart-parity.test.ts` 读。两份实现各写一遍
//! multipart 规则（boundary 怎么生成、Content-Type 放哪、用户自己写过的
//! Content-Type 怎么处理），只有一份共享样本才能钉住它们没分叉。
//!
//! boundary 每次请求现生成，两侧各生成各的，故样本里写占位符 `<BOUNDARY>`：
//! 读的时候各自把自己那个 boundary 换成占位符再比对。

use serde::Deserialize;

use super::multipart;
use crate::variable_fill::{resolve_endpoint, EndpointOverrides};
use simple_post_core::domain::{
    AuthConfig, BodyMode, EndpointDetail, Environment, KeyValue, Project, RequestBody,
};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../../fixtures/multipart-parity.json");

const BOUNDARY_PLACEHOLDER: &str = "<BOUNDARY>";

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    env: std::collections::BTreeMap<String, String>,
    input: Input,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    method: String,
    url_path: String,
    base_url: String,
    #[serde(default)]
    params: Vec<KeyValue>,
    #[serde(default)]
    headers: Vec<KeyValue>,
    body: RequestBody,
    auth: AuthConfig,
}

#[derive(Debug, Deserialize)]
struct Expected {
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn detail(input: &Input) -> EndpointDetail {
    EndpointDetail {
        node_id: "e1".into(),
        method: simple_post_core::domain::parse_http_method(&input.method)
            .expect("样本里的 method 必须是合法的 HTTP 动词"),
        url: input.url_path.clone(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: Some(input.base_url.clone()),
        params: input.params.clone(),
        headers: input.headers.clone(),
        body: input.body.clone(),
        auth: input.auth.clone(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    }
}

fn env(variables: &std::collections::BTreeMap<String, String>) -> Environment {
    Environment {
        id: "env1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: Default::default(),
        module_base_urls: Default::default(),
        variables: variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        disabled_variables: vec![],
        is_active: true,
    }
}

/// 从 Content-Type 里取出这次请求的 boundary。
fn boundary_of(headers: &[(String, String)]) -> String {
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .expect("multipart 请求必须带 Content-Type");
    ct.1.strip_prefix("multipart/form-data; boundary=")
        .expect("Content-Type 必须是 multipart/form-data 且带 boundary")
        .to_string()
}

/// 把这次请求的 boundary 换成占位符，让两侧的产出可比。
fn normalize(text: &str, boundary: &str) -> String {
    text.replace(boundary, BOUNDARY_PLACEHOLDER)
}

/// 两侧对同一份 multipart 请求要装配出逐字节相同的结果。
#[test]
fn every_multipart_parity_case_assembles_the_same_way() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("parity fixture");
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    for case in &fixture.cases {
        let detail = detail(&case.input);
        let env = env(&case.env);
        let project = Project {
            id: "p1".into(),
            name: "P".into(),
            description: None,
            base_url: None,
            base_url_definitions: vec![],
            default_base_url_definition_id: None,
            color: String::new(),
            created_at: 0,
            updated_at: 0,
        };

        let resolved = resolve_endpoint(
            &detail,
            Some(&env),
            &project,
            Some(&EndpointOverrides::default()),
        )
        .unwrap_or_else(|e| panic!("{}: 解析失败: {e}", case.name));

        assert_eq!(resolved.url, case.expected.url, "{}: URL", case.name);

        // `resolve_endpoint` 只做变量替换；multipart 的装配是发送方那一步的事
        // （boundary 现生成，Content-Type 与 body 用同一个值）。
        let mut headers = resolved.headers.clone();
        let content = resolved
            .body
            .clone()
            .unwrap_or_else(|| panic!("{}: body 解析后为空", case.name));
        let body = multipart::apply(&mut headers, &content);

        let boundary = boundary_of(&headers);
        let got_headers: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.clone(), normalize(v, &boundary)))
            .collect();
        assert_eq!(got_headers, case.expected.headers, "{}: header", case.name);
        assert_eq!(
            normalize(&body, &boundary),
            case.expected.body,
            "{}: body",
            case.name
        );
    }
}

/// 样本里的 body 模式必须真是 multipart：否则这条测试在验一件别的事。
#[test]
fn the_fixture_uses_the_multipart_body_mode() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("parity fixture");
    for case in &fixture.cases {
        assert_eq!(
            case.input.body.mode,
            BodyMode::Multipart,
            "{}: body 模式必须是 multipart",
            case.name
        );
    }
}
