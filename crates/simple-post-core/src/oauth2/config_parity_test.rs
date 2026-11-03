//! OAuth2 配置的 `{{var}}` 替换规则的共享样本读取器。
//!
//! 判据来自共享样本 `fixtures/oauth2-config-parity.json`，渲染进程那一侧
//! （`src/lib/__tests__/oauth2-config-parity.test.ts`）读同一份文件。
//!
//! 替换规则只有 core 这一份实现（[`super::config::substitute_config`]），故本读取器验的
//! 是**规则本身**：哪些字段被替换、哪些不替换、缺变量时怎么报。渲染进程那侧不再自己
//! 替换，它只需要知道同一份字段清单（`OAUTH2_SUBSTITUTED_FIELDS`），故两个读取器比的是
//! 同一份样本的同一件事。

use std::collections::HashMap;

use serde::Deserialize;

use crate::domain::OAuth2Auth;

use super::config::{substitute_config, SubstitutedConfig, KEPT_FIELDS, SUBSTITUTED_FIELDS};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/oauth2-config-parity.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Fixture {
    substituted_fields: Vec<String>,
    kept_fields: Vec<String>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Case {
    name: String,
    vars: HashMap<String, String>,
    config: Config,
    expected: Config,
    missing: Vec<String>,
}

/// 样本里用到的 OAuth2 字段。
///
/// 刻意只列规则谈到的那几个字段：样本写别的字段会被 `deny_unknown_fields` 挡下来，
/// 于是「哪些字段参与替换」这条规则不会因为样本自己扩了字段而悄悄变宽。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Config {
    #[serde(default)]
    token_url: String,
    #[serde(default)]
    authorization_url: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    auth_code: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    code_verifier: String,
}

impl Config {
    fn to_auth(&self) -> OAuth2Auth {
        OAuth2Auth {
            token_url: self.token_url.clone(),
            authorization_url: self.authorization_url.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            scope: self.scope.clone(),
            auth_code: self.auth_code.clone(),
            password: self.password.clone(),
            username: self.username.clone(),
            redirect_uri: self.redirect_uri.clone(),
            code_verifier: self.code_verifier.clone(),
            ..OAuth2Auth::default()
        }
    }
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("oauth2 config parity fixture")
}

/// 逐个字段取出来比。逐字段比而不是整体比：整体比只说「不相等」，逐字段比能指出
/// 差的是哪个字段。
fn fields(cfg: &OAuth2Auth) -> Vec<(&'static str, &str)> {
    vec![
        ("tokenUrl", cfg.token_url.as_str()),
        ("authorizationUrl", cfg.authorization_url.as_str()),
        ("clientId", cfg.client_id.as_str()),
        ("clientSecret", cfg.client_secret.as_str()),
        ("scope", cfg.scope.as_str()),
        ("authCode", cfg.auth_code.as_str()),
        ("password", cfg.password.as_str()),
        ("username", cfg.username.as_str()),
        ("redirectUri", cfg.redirect_uri.as_str()),
        ("codeVerifier", cfg.code_verifier.as_str()),
    ]
}

/// 缺变量清单摊平成一个列表：按字段顺序，字段内按首次出现顺序，跨字段去重。
fn flattened_missing(sub: &SubstitutedConfig) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, names) in &sub.missing {
        for n in names {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
    }
    out
}

#[test]
fn every_case_matches_the_shared_sample() {
    let fixture = fixture();
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    // 先把全部不符收集起来再一次性断言：改一处能看到全部受影响的地方。
    let mut mismatches: Vec<String> = Vec::new();
    for case in &fixture.cases {
        let sub = substitute_config(&case.config.to_auth(), &case.vars);
        let expected = case.expected.to_auth();

        for ((field, got), (_, want)) in fields(&sub.config).iter().zip(fields(&expected).iter()) {
            if got != want {
                mismatches.push(format!("{}: {field} 是 {got}，样本要求 {want}", case.name));
            }
        }

        let missing = flattened_missing(&sub);
        if missing != case.missing {
            mismatches.push(format!(
                "{}: 缺变量清单是 {missing:?}，样本要求 {:?}",
                case.name, case.missing
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "这些 case 与样本对不上：\n{}",
        mismatches.join("\n")
    );
}

/// 样本本身得覆盖得住替换规则的每一条。
#[test]
fn the_sample_covers_the_substitution_rule() {
    let fixture = fixture();
    let names: Vec<&str> = fixture.cases.iter().map(|c| c.name.as_str()).collect();

    let needed = [
        "地址与标识被替换，密钥与一次性输入不替换",
        "变量池里没有的变量原样留着，并报成缺变量",
        "密钥与一次性输入里的变量不替换，也不报缺变量",
        "标记名不是裸标识符时原样留着",
        "变量池为空时四个被替换的字段都原样留着",
        "变量池里的取值为空串时算已解析",
    ];

    for one in needed {
        assert!(
            names.contains(&one),
            "样本里缺了这条规则，两侧就可能在这里分叉：{one}"
        );
    }

    // 反向也查一遍：新增一条 case 却忘了登记进上面那份名单时，样本与名单就会各说各话，
    // 而这条用例仍然全绿。
    let unregistered: Vec<&str> = names
        .iter()
        .filter(|n| !needed.contains(n))
        .copied()
        .collect();
    assert!(
        unregistered.is_empty(),
        "样本里有未登记的 case：{}",
        unregistered.join("、")
    );
}

/// 被替换与不被替换的字段清单由样本说了算，两份清单必须逐条相同。
#[test]
fn the_field_lists_match_the_shared_sample() {
    let fixture = fixture();

    assert_eq!(
        SUBSTITUTED_FIELDS.to_vec(),
        fixture.substituted_fields,
        "被替换的字段清单与样本不一致"
    );
    assert_eq!(
        KEPT_FIELDS.to_vec(),
        fixture.kept_fields,
        "刻意不替换的字段清单与样本不一致"
    );

    // 两份清单不能有交集：一个字段同时在两份清单里，规则就自相矛盾了。
    for field in SUBSTITUTED_FIELDS {
        assert!(
            !KEPT_FIELDS.contains(&field),
            "{field} 同时出现在两份清单里"
        );
    }
}
