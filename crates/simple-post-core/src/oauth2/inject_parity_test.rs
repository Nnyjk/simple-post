//! OAuth2 token 注入的共享样本读取器。
//!
//! 判据来自共享样本 `fixtures/oauth2-parity.json`，渲染进程那一侧
//! （`src/lib/__tests__/oauth2-parity.test.ts`）读同一份文件。两份实现各写一遍规则，
//! 只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没人提醒另一边。
//!
//! 样本比的是**结果**（注入后的 URL 与请求头列表），不是写法——本 crate 就地改
//! `headers` 与 `url`，渲染进程侧返回新对象。

use serde::Deserialize;

use crate::domain::{OAuth2Auth, OAuth2TokenLocation};

use super::inject::inject;
use super::OAuth2Token;

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/oauth2-parity.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    token_location: String,
    token_param: String,
    token: TokenCase,
    headers: Vec<(String, String)>,
    url: String,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenCase {
    access_token: String,
    token_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    url: String,
    headers: Vec<(String, String)>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("oauth2 parity fixture")
}

/// 样本里的 `tokenLocation` 是字符串，这里把它对回枚举。样本写错时立刻炸，不静默放过。
fn location_of(name: &str) -> OAuth2TokenLocation {
    match name {
        "header" => OAuth2TokenLocation::Header,
        "query" => OAuth2TokenLocation::Query,
        other => panic!("样本里的 tokenLocation 取值不认识：{other}"),
    }
}

fn auth(case: &Case) -> OAuth2Auth {
    OAuth2Auth {
        token_location: location_of(&case.token_location),
        token_param: case.token_param.clone(),
        ..Default::default()
    }
}

fn token(case: &TokenCase) -> OAuth2Token {
    OAuth2Token {
        access_token: case.access_token.clone(),
        token_type: case.token_type.clone(),
        refresh_token: None,
        scope: None,
        expires_at: None,
        obtained_at: 0,
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
        let mut headers = case.headers.clone();
        let mut url = case.url.clone();
        inject(&auth(case), &token(&case.token), &mut headers, &mut url);

        assert_eq!(url, case.expected.url, "{}: 注入后的 URL", case.name);
        assert_eq!(
            headers, case.expected.headers,
            "{}: 注入后的请求头",
            case.name
        );
    }
}

/// 样本本身得覆盖得住注入规则的每一条。
#[test]
fn the_sample_covers_the_injection_rules() {
    let fixture = fixture();
    let names: Vec<&str> = fixture.cases.iter().map(|c| c.name.as_str()).collect();

    for needed in [
        "header 档补一条 Authorization",
        "header 档的 token 类型是空串时只放裸 token",
        "header 档原样透传 token 类型不改成 Bearer",
        "header 档不覆盖用户自己写的 Authorization",
        "header 档认大小写不敏感的 authorization",
        "query 档把 token 追加到已有的查询串后面",
        "query 档给没有查询串的地址起一个查询串",
        "query 档的参数名是空白时回落成 access_token",
        "query 档用配置的那个参数名",
        "query 档的 token 值按查询串的字符集编码",
        "query 档的参数名也按查询串的字符集编码",
    ] {
        assert!(
            names.contains(&needed),
            "样本里缺了这条规则，两侧就可能在这里分叉：{needed}"
        );
    }
}
