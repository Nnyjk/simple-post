//! base URL 服务链解析的共享样本读取器。
//!
//! 判据来自共享样本 `fixtures/base-url-parity.json`，渲染进程那一侧
//! （`src/lib/__tests__/base-url-parity.test.ts`）读同一份文件。两份实现各写一遍规则，
//! 只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没人提醒另一边。
//!
//! 样本只覆盖两侧**共有**的那部分规则。Rust 侧多一个 `forced_def_id`（调用方点名服务，
//! MCP 侧用得上），渲染进程侧没有这个概念，故它留在本 crate 自己的测试里，不进样本。

use std::collections::HashMap;

use serde::Deserialize;

use simple_post_core::domain::{
    AuthConfig, BaseUrlDefinition, EndpointDetail, Environment, HttpMethod, Project, RequestBody,
};

use super::base_url::{resolve_base_url_with, BaseUrlSource};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/base-url-parity.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    endpoint: EndpointCase,
    env: Option<EnvCase>,
    project: ProjectCase,
    module_id: Option<String>,
    expected: Expected,
}

/// 端点上解析真正读的那两个字段。环境由调用方给出，不从端点上取。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndpointCase {
    base_url_definition_id: Option<String>,
    base_url_override: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvCase {
    module_base_urls: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectCase {
    base_url_definitions: Vec<DefinitionCase>,
    default_base_url_definition_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DefinitionCase {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    base_url: String,
    source: String,
    definition_id: Option<String>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("base url parity fixture")
}

/// 样本里的 `source` 是字符串，这里把它对回枚举。样本写错时立刻炸，不静默放过。
fn source_of(name: &str) -> BaseUrlSource {
    match name {
        "override" => BaseUrlSource::Override,
        "definition" => BaseUrlSource::Definition,
        "default" => BaseUrlSource::Default,
        "none" => BaseUrlSource::None,
        other => panic!("样本里的 source 取值不认识：{other}"),
    }
}

fn detail(case: &EndpointCase) -> EndpointDetail {
    EndpointDetail {
        node_id: "ep1".into(),
        method: HttpMethod::Get,
        url: "/users".into(),
        environment_id: None,
        base_url_definition_id: case.base_url_definition_id.clone(),
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
    }
}

fn env(case: Option<&EnvCase>) -> Option<Environment> {
    case.map(|c| Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        module_base_urls: c.module_base_urls.clone(),
        variables: HashMap::new(),
        disabled_variables: Vec::new(),
        is_active: true,
    })
}

fn project(case: &ProjectCase) -> Project {
    Project {
        id: "p1".into(),
        name: "demo".into(),
        description: None,
        base_url: None,
        base_url_definitions: case
            .base_url_definitions
            .iter()
            .map(|d| BaseUrlDefinition {
                id: d.id.clone(),
                name: d.name.clone(),
                description: None,
            })
            .collect(),
        default_base_url_definition_id: case.default_base_url_definition_id.clone(),
        color: "#000".into(),
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn every_case_matches_the_shared_sample() {
    let fixture = fixture();
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    for case in &fixture.cases {
        // `forced_def_id` 传 `None`：渲染进程侧没有这个概念，样本覆盖的是两侧共有的规则。
        let (url, source, def) = resolve_base_url_with(
            &detail(&case.endpoint),
            env(case.env.as_ref()).as_ref(),
            &project(&case.project),
            None,
            case.module_id.as_deref(),
        );
        assert_eq!(
            url, case.expected.base_url,
            "{}: 解析出的 base URL",
            case.name
        );
        assert_eq!(
            source,
            source_of(&case.expected.source),
            "{}: 哪一行选中了服务",
            case.name
        );
        assert_eq!(def, case.expected.definition_id, "{}: 服务 id", case.name);
    }
}

/// 样本本身得覆盖得住服务链的每一条规则。
#[test]
fn the_sample_covers_the_resolution_rules() {
    let fixture = fixture();
    let names: Vec<&str> = fixture.cases.iter().map(|c| c.name.as_str()).collect();

    for needed in [
        "端点带自定义主机时用自己的地址",
        "端点的 override 是空串时表示自定义但不加前缀",
        "端点没给 override 时走服务链",
        "端点点名的服务项目里没有时不回退到项目星标",
        "端点点名的服务项目里没有时即使环境里有那一格也不解析",
        "端点没点名服务时用项目星标",
        "项目星标指向的服务项目里没有时解析不出东西",
        "项目没有星标且端点没点名服务时解析不出东西",
        "那一格是空串时解析不出东西",
        "服务存在但环境里没有那一格时解析不出东西",
        "端点自己点名的服务胜过项目星标",
        "同一个服务在两个模块指向两个主机",
        "模块未知时没有格子可读",
        "环境缺席时解析不出东西",
        "端点点名的服务是空串时解析不出东西",
    ] {
        assert!(
            names.contains(&needed),
            "样本里缺了这条规则，两侧就可能在这里分叉：{needed}"
        );
    }
}
