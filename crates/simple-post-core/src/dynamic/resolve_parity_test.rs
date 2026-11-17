//! 动态变量规则的跨语言样本：Rust 的 `core::dynamic`。
//!
//! 同一份样本也被 TS 侧的 `src/lib/__tests__/dynamic-parity.test.ts` 读。两份实现各写一遍
//! 规则（变量池的合并、值的序列化、哪些名字算被引用），只有一份共享样本才能钉住它们没分叉。

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::Value;

use super::{list_names, merge_pool, serialize_value, TemplateParts};
use crate::domain::{AuthConfig, KeyValue, RequestBody, ValueMode};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/dynamic-parity.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    /// 夹具文件自带的人类说明，两侧读者都不用它；声明下来只是为了不让
    /// `deny_unknown_fields` 把它当成夹具写错的键。
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    merge_cases: Vec<MergeCase>,
    serialize_cases: Vec<SerializeCase>,
    scan_cases: Vec<ScanCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeCase {
    name: String,
    env: BTreeMap<String, String>,
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default)]
    dynamic: BTreeMap<String, String>,
    expected: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SerializeCase {
    name: String,
    mode: String,
    value: Value,
    expected: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanCase {
    name: String,
    parts: ScanParts,
    expected: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScanParts {
    #[serde(default)]
    base_url: String,

    #[serde(default)]
    url: String,
    #[serde(default)]
    params: Vec<KeyValue>,
    #[serde(default)]
    path_params: Vec<KeyValue>,
    #[serde(default)]
    headers: Vec<KeyValue>,
    #[serde(default)]
    body: RequestBody,
    #[serde(default)]
    auth: AuthConfig,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("parity fixture")
}

#[test]
fn every_merge_case_merges_the_same_way() {
    let fixture = fixture();
    assert_eq!(
        fixture.merge_cases.len(),
        5,
        "合并规则样本必须恰好 5 条：条数变了说明夹具动了，两侧读取器都得跟着看"
    );

    for case in &fixture.merge_cases {
        let env: HashMap<String, String> = case
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let dynamic: HashMap<String, String> = case
            .dynamic
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let got = merge_pool(&env, &case.disabled, &dynamic);

        let mut got_pairs: Vec<(String, String)> = got.into_iter().collect();
        got_pairs.sort();
        let mut want_pairs: Vec<(String, String)> = case
            .expected
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        want_pairs.sort();

        assert_eq!(got_pairs, want_pairs, "{}: 变量池", case.name);
    }
}

#[test]
fn every_serialize_case_produces_the_same_text() {
    let fixture = fixture();
    assert_eq!(
        fixture.serialize_cases.len(),
        15,
        "序列化样本必须恰好 15 条：条数变了说明夹具动了，两侧读取器都得跟着看"
    );

    for case in &fixture.serialize_cases {
        let mode = ValueMode::parse(&case.mode)
            .unwrap_or_else(|| panic!("{}: 未知的 value_mode '{}'", case.name, case.mode));
        let got = serialize_value(&case.value, mode);
        assert_eq!(got, case.expected, "{}: 序列化", case.name);
    }
}

#[test]
fn every_scan_case_finds_the_same_names() {
    let fixture = fixture();
    assert_eq!(
        fixture.scan_cases.len(),
        11,
        "名字扫描样本必须恰好 11 条：条数变了说明夹具动了，两侧读取器都得跟着看"
    );

    for case in &fixture.scan_cases {
        let parts = TemplateParts {
            base_url: &case.parts.base_url,

            url: &case.parts.url,
            params: &case.parts.params,
            path_params: &case.parts.path_params,
            headers: &case.parts.headers,
            body: &case.parts.body,
            auth: &case.parts.auth,
        };
        let mut got = list_names(&parts);
        got.sort();
        let mut want = case.expected.clone();
        want.sort();

        assert_eq!(got, want, "{}: 被引用的名字", case.name);
    }
}

/// 样本本身得是有效的：`text` 与 `auto` 的输出逐例相同，这是用户拍板的结论，由样本钉住。
#[test]
fn the_fixture_pins_text_and_auto_to_the_same_output() {
    let fixture = fixture();
    let mut by_name: HashMap<String, String> = HashMap::new();
    for case in &fixture.serialize_cases {
        by_name.insert(case.name.clone(), case.mode.clone());
    }
    let auto = fixture
        .serialize_cases
        .iter()
        .find(|c| c.mode == "auto" && c.expected.starts_with('['))
        .expect("样本里有一个 auto 的数组用例");
    let text = fixture
        .serialize_cases
        .iter()
        .find(|c| c.mode == "text" && c.expected.starts_with('['))
        .expect("样本里有一个 text 的数组用例");
    assert_eq!(
        auto.expected, text.expected,
        "text 与 auto 对数组的输出必须一致"
    );
    assert!(by_name.contains_key("auto 对字符串给纯文本"));
}
