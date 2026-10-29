//! 两侧请求准备规则的一致性样本：Rust 的 `resolve_endpoint`（MCP 端）。
//!
//! 同一份样本也被 TS 侧的 `src/lib/__tests__/http-build-parity.test.ts` 读。
//! 两份实现各写一遍规则（baseUrl 拼接、`{{var}}` 替换、收斜杠、query、header 排序、
//! 鉴权派生、body「先判空后模板化」），只有一份共享样本才能钉住它们没分叉。

use serde::Deserialize;
use serde_json::Value;

use super::resolve_endpoint;
use super::types::EndpointOverrides;
use simple_post_core::domain::{
    AuthConfig, BodyMode, EndpointDetail, Environment, KeyValue, RequestBody,
};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/resolve-parity.json");

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
    path_params: Vec<KeyValue>,
    #[serde(default)]
    headers: Vec<KeyValue>,
    body: RequestBody,
    auth: AuthConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    /// 未解析出来的变量名。比的是**集合**而非顺序：两侧收集的次序各自是本地的
    /// （渲染层按它渲染告警胶囊，MCP 侧按它报 `_missing_vars`），契约只承诺
    /// 「哪些变量没解析出来」。
    #[serde(default)]
    missing_vars: Vec<String>,
    /// URL 路径里没取到值的占位符名。比的是**集合**而非顺序，与未解析变量同一条纪律。
    #[serde(default)]
    missing_path_params: Vec<String>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("parity fixture")
}

/// 样本里的端点行。`base_url_override` 就是「自定义模式」的 base URL。
fn detail(input: &Input) -> EndpointDetail {
    EndpointDetail {
        node_id: "e1".into(),
        method: simple_post_core::domain::parse_http_method(&input.method)
            .expect("样本里的 method 必须是合法的 HTTP 动词"),
        url: input.url_path.clone(),
        environment_id: None,
        base_url_override: Some(input.base_url.clone()),
        params: input.params.clone(),
        path_params: input.path_params.clone(),
        headers: input.headers.clone(),
        body: input.body.clone(),
        auth: input.auth.clone(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    }
}

fn env(variables: &std::collections::BTreeMap<String, String>) -> Environment {
    Environment {
        id: "env1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        module_base_urls: Default::default(),
        variables: variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        disabled_variables: vec![],
        is_active: true,
    }
}

#[test]
fn every_parity_case_resolves_the_same_way() {
    let fixture = fixture();
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    for case in &fixture.cases {
        let detail = detail(&case.input);
        let env = env(&case.env);
        // 样本的 base URL 走端点自己的 override，故环境里没有格子也不影响。
        let resolved = resolve_endpoint(&detail, Some(&env), Some(&EndpointOverrides::default()))
            .unwrap_or_else(|e| panic!("{}: 解析失败: {e}", case.name));

        assert_eq!(resolved.url, case.expected.url, "{}: URL", case.name);
        assert_eq!(
            resolved.headers, case.expected.headers,
            "{}: header",
            case.name
        );
        assert_eq!(resolved.body, case.expected.body, "{}: body", case.name);

        let mut got = resolved.missing_vars.clone();
        let mut want = case.expected.missing_vars.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{}: 未解析的变量", case.name);

        let mut got_params = resolved.missing_path_params.clone();
        let mut want_params = case.expected.missing_path_params.clone();
        got_params.sort();
        want_params.sort();
        assert_eq!(got_params, want_params, "{}: 缺值的路径参数", case.name);
    }
}

/// 样本本身得是有效的：body 模式与 Content-Type 对得上，否则样本会教坏两边。
#[test]
fn the_fixture_matches_the_content_type_rules() {
    let fixture = fixture();
    for case in &fixture.cases {
        let has_content_type = case
            .expected
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        let expect_content_type = matches!(
            case.input.body.mode,
            BodyMode::Json | BodyMode::Form | BodyMode::Raw
        );
        assert_eq!(
            has_content_type, expect_content_type,
            "{}: body 模式与 Content-Type 对不上",
            case.name
        );
    }
}

/// 样本用的 JSON 形状必须与领域类型一致（否则解析悄悄用默认值而不是报错）。
#[test]
fn the_fixture_uses_the_real_wire_shapes() {
    let raw: Value = serde_json::from_str(FIXTURE).expect("fixture is json");
    let first = &raw["cases"][0];
    assert!(
        first["input"]["params"][0].get("id").is_some(),
        "KeyValue.id 必填"
    );
    assert_eq!(first["input"]["method"], "POST");
    assert!(first["input"]["body"].get("mode").is_some());
}
