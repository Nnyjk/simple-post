//! 请求体解析规则的一致性样本：Rust 的 `core::body`（MCP 端）。
//!
//! 同一份样本也被 TS 侧的 `src/lib/__tests__/body-parity.test.ts` 读。两份实现各写
//! 一遍规则（逐字段还是逐段模板化、空键与停用行算不算、百分号编码用哪套字符集、
//! 产物是文本还是字节），只有一份共享样本才能钉住它们没分叉。

use std::collections::HashMap;

use serde::Deserialize;

use crate::body::{binary_content_type, resolve_body};
use crate::domain::{BodyMode, RequestBody};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../fixtures/body-parity.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    #[serde(default)]
    vars: HashMap<String, String>,
    body: RequestBody,
    expected: Expected,
    /// `binary` 模式的 Content-Type。缺席即不检查这一项。
    #[serde(default)]
    expected_content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    text: Option<String>,
    base64: Option<String>,
    #[serde(default)]
    missing: Vec<String>,
}

#[test]
fn every_body_parity_case_resolves_the_same_way() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("body parity fixture");
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    for case in &fixture.cases {
        let resolved = resolve_body(&case.body, &case.vars);

        assert_eq!(resolved.text, case.expected.text, "{}: 文本形态", case.name);
        assert_eq!(
            resolved.base64, case.expected.base64,
            "{}: 二进制形态",
            case.name
        );
        assert_eq!(
            resolved.missing, case.expected.missing,
            "{}: 未解析的变量",
            case.name
        );
    }
}

/// `binary` 模式的 Content-Type 取文件自己的类型。样本里写死了期望值，两侧都要对得上。
#[test]
fn every_body_parity_case_has_the_same_binary_content_type() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("body parity fixture");
    for case in &fixture.cases {
        let Some(expected) = &case.expected_content_type else {
            continue;
        };
        assert_eq!(
            binary_content_type(&case.body.content),
            *expected,
            "{}: binary 的 Content-Type",
            case.name
        );
    }
}

/// 样本本身得是有效的：body 模式必须是合法取值，且样本里至少有一个 `binary` 用例
/// （没有它，Content-Type 那条规则就没有被钉住）。
#[test]
fn the_fixture_uses_the_real_body_modes() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("body parity fixture");
    let modes: Vec<BodyMode> = fixture.cases.iter().map(|c| c.body.mode.clone()).collect();
    assert!(
        modes.contains(&BodyMode::Binary),
        "样本里必须有一个 binary 用例"
    );
    assert!(
        modes.contains(&BodyMode::Multipart),
        "样本里必须有一个 multipart 用例"
    );
}
