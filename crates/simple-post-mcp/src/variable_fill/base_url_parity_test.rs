//! base URL 解析与代理前缀替换的共享样本读取器。
//!
//! 判据来自共享样本 `fixtures/base-url-parity.json`，渲染进程那一侧
//! （`src/lib/__tests__/base-url-parity.test.ts`）读同一份文件。两份实现各写一遍规则，
//! 只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没人提醒另一边。

use std::collections::HashMap;

use serde::Deserialize;

use simple_post_core::domain::{
    AuthConfig, EndpointDetail, Environment, HttpMethod, ModuleBaseUrl, RequestBody,
};

use super::base_url::{apply_proxy, resolve_base_url, BaseUrlSource};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/base-url-parity.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    /// 样本自己的说明文字，不是数据。渲染进程那一侧也把它列进顶层允许的键里。
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    cases: Vec<Case>,
    proxy_cases: Vec<ProxyCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    endpoint: EndpointCase,
    env: Option<EnvCase>,
    module_id: Option<String>,
    expected: Expected,
}

/// 端点上解析真正读的那一个字段。环境由调用方给出，不从端点上取。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndpointCase {
    base_url_override: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnvCase {
    module_base_urls: HashMap<String, CellCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CellCase {
    url: String,
    #[serde(default)]
    proxy: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Expected {
    base_url: String,
    #[serde(default)]
    proxy: Option<String>,
    source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProxyCase {
    name: String,
    base_url: String,
    #[serde(default)]
    proxy: Option<String>,
    url: String,
    expected: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("base url parity fixture")
}

/// 样本里的 `source` 是字符串，这里把它对回枚举。样本写错时立刻炸，不静默放过。
fn source_of(name: &str) -> BaseUrlSource {
    match name {
        "override" => BaseUrlSource::Override,
        "module" => BaseUrlSource::Module,
        "none" => BaseUrlSource::None,
        other => panic!("样本里的 source 取值不认识：{other}"),
    }
}

fn detail(case: &EndpointCase) -> EndpointDetail {
    EndpointDetail {
        node_id: "ep1".into(),
        method: HttpMethod::Get,
        url: "/users".into(),
        base_url_override: case.base_url_override.clone(),
        params: Vec::new(),
        path_params: Vec::new(),
        headers: Vec::new(),
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    }
}

fn env(case: Option<&EnvCase>) -> Option<Environment> {
    case.map(|c| Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        module_base_urls: c
            .module_base_urls
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ModuleBaseUrl {
                        url: v.url.clone(),
                        proxy: v.proxy.clone(),
                    },
                )
            })
            .collect(),
        variables: HashMap::new(),
        disabled_variables: Vec::new(),
        created_at: 0,
    })
}

#[test]
fn every_case_matches_the_shared_sample() {
    let fixture = fixture();
    // 条数写死：样本的键名漂了或样本被改小，这条断言也得红。写成「非空」的话，
    // 上面那几行 `if` 一条都走不到，测试还是绿的。
    assert_eq!(
        fixture.cases.len(),
        11,
        "样本里的 case 条数变了。改样本就要改这个数字，并确认渲染进程那一侧的读取器一起改"
    );

    // 先把全部不符收集起来再一次性断言：`assert_eq!` 失败就 panic，后面的 case 就不再
    // 跑了——修的人会以为只坏了一处规则。收集完再报，改一处能看到全部受影响的地方。
    let mut mismatches: Vec<String> = Vec::new();
    for case in &fixture.cases {
        let got = resolve_base_url(
            &detail(&case.endpoint),
            env(case.env.as_ref()).as_ref(),
            case.module_id.as_deref(),
        );
        if got.base_url != case.expected.base_url {
            mismatches.push(format!(
                "{}: base URL 是 {}，样本要求 {}",
                case.name, got.base_url, case.expected.base_url
            ));
        }
        if got.proxy != case.expected.proxy {
            mismatches.push(format!(
                "{}: 代理是 {:?}，样本要求 {:?}",
                case.name, got.proxy, case.expected.proxy
            ));
        }
        if got.source != source_of(&case.expected.source) {
            mismatches.push(format!(
                "{}: 来源是 {:?}，样本要求 {}",
                case.name, got.source, case.expected.source
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "这些 case 与样本对不上：\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn every_proxy_case_matches_the_shared_sample() {
    let fixture = fixture();
    assert_eq!(
        fixture.proxy_cases.len(),
        9,
        "样本里的 proxyCase 条数变了。改样本就要改这个数字，并确认渲染进程那一侧的读取器一起改"
    );

    let mut mismatches: Vec<String> = Vec::new();
    for case in &fixture.proxy_cases {
        let got = apply_proxy(&case.base_url, case.proxy.as_deref(), &case.url);
        if got != case.expected {
            mismatches.push(format!(
                "{}: 得到 {got}，样本要求 {}",
                case.name, case.expected
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "这些 proxyCase 与样本对不上：\n{}",
        mismatches.join("\n")
    );
}

/// 样本本身得覆盖得住每一条规则。
///
/// 这一条是防「读取器的键名与样本对不上」的那道防线：serde 遇到不认识的键会**静默
/// 忽略**，于是 `expected.proxy` 永远读到 `None`，上面两条断言一条用例都没走到，却
/// 一直是绿的。故这里数一遍样本里真正带代理的条目，为 0 就失败。
#[test]
fn the_sample_covers_the_resolution_rules() {
    let fixture = fixture();

    let with_proxy = fixture
        .cases
        .iter()
        .filter(|c| c.expected.proxy.is_some())
        .count();
    assert!(
        with_proxy > 0,
        "样本里没有一条带代理的 case，代理这一条规则就没人钉住"
    );

    let proxy_cases_with_proxy = fixture
        .proxy_cases
        .iter()
        .filter(|c| c.proxy.is_some())
        .count();
    assert!(
        proxy_cases_with_proxy > 0,
        "样本里没有一条带代理的 proxyCase，前缀替换就没人钉住"
    );

    let override_cases = fixture
        .cases
        .iter()
        .filter(|c| c.expected.source == "override")
        .count();
    assert!(
        override_cases > 0,
        "样本里没有一条端点自带地址的 case，override 那一档就没人钉住"
    );

    let module_cases = fixture
        .cases
        .iter()
        .filter(|c| c.expected.source == "module")
        .count();
    assert!(
        module_cases > 0,
        "样本里没有一条走模块格子的 case，那一档就没人钉住"
    );

    let none_cases = fixture
        .cases
        .iter()
        .filter(|c| c.expected.source == "none")
        .count();
    assert!(
        none_cases > 0,
        "样本里没有一条解析不出东西的 case，空格子那一档就没人钉住"
    );
}
