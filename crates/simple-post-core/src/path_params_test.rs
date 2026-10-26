//! 路径占位符的识别范围与替换规则。
//!
//! 判据来自共享样本 `fixtures/path-params-parity.json`，渲染进程那一侧
//! （`src/lib/__tests__/path-params.test.ts`）读同一份文件。两份实现各写一遍规则，
//! 只有共享样本能钉住它们没分叉：手写的镜像测试各有两份，改了一边就没人提醒另一边。

use serde::Deserialize;

use super::{placeholders, substitute};

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../fixtures/path-params-parity.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    url: String,
    /// 这段 URL 里认得出的占位符名，按出现顺序。
    placeholders: Vec<String>,
    /// 「占位符名 → 取到的值」。空列表表示没有值可取。
    #[serde(default)]
    values: Vec<(String, String)>,
    /// 替换之后的 URL。
    expected_url: String,
    /// 取不到值的占位符名。
    #[serde(default)]
    expected_missing: Vec<String>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("path params parity fixture")
}

#[test]
fn every_case_matches_the_shared_sample() {
    let fixture = fixture();
    assert!(
        !fixture.cases.is_empty(),
        "样本里必须有 case，否则这条测试是空的"
    );

    for case in &fixture.cases {
        assert_eq!(
            placeholders(&case.url),
            case.placeholders,
            "{}: 占位符名单",
            case.name
        );

        let (url, missing) = substitute(&case.url, &case.values);
        assert_eq!(url, case.expected_url, "{}: 替换后的 URL", case.name);

        // 比的是**集合**而非顺序：两侧收集缺值的次序各自是本地的，契约只承诺
        // 「哪些占位符没取到值」。
        let mut got = missing;
        let mut want = case.expected_missing.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{}: 缺值的占位符", case.name);
    }
}

/// 样本本身得覆盖得住「识别范围」这半边规则。
///
/// 替换那半边由 `fixtures/resolve-parity.json` 覆盖（那边比的是整条请求的装配结果），
/// 这边比的是「哪些 `{…}` 算占位符」——两条规则各有一份样本，改了一边就没人提醒另一边。
#[test]
fn the_sample_covers_the_identification_scope() {
    let fixture = fixture();
    let names: Vec<&str> = fixture.cases.iter().map(|c| c.name.as_str()).collect();

    for needed in [
        "路径里的一对大括号就是一个占位符",
        "双大括号是环境变量的写法，不是路径占位符",
        "查询串里的占位符不是路径占位符",
        "主机名里的占位符不是路径占位符",
        "未闭合的大括号不算占位符",
        "名字为空不算占位符",
        "名字里有空格不算占位符",
        "名字里有制表符不算占位符",
        "名字里有不换行空格不算占位符",
        "名字里有 NEL 不算占位符",
        "名字里有斜杠不算占位符",
        "名字里有与号不算占位符",
        "U+FEFF 不在 Unicode 空白集合里，两侧都把它当名字的一部分",
    ] {
        assert!(
            names.contains(&needed),
            "样本里缺了「{needed}」这条识别规则，两侧就可能在这里分叉"
        );
    }
}

/// 空白集合的差集在两侧的判据里恰好相反，故这两条断言写在两边。
///
/// Rust 的 `char::is_whitespace` 含 U+0085（NEL）不含 U+FEFF；JS 的 `\s` 含 U+FEFF
/// 不含 U+0085。两边用不同的集合，同一段 URL 在界面与 MCP 侧就会认出不同的占位符
/// 名单。渲染进程那侧写了同款断言（见 `src/lib/__tests__/path-params.test.ts`）。
#[test]
fn the_whitespace_set_is_the_unicode_white_space_property() {
    // U+0085 是空白 → 名字里带它就不是一个合法的占位符名。
    assert!(placeholders("/users/{a\u{0085}b}").is_empty());
    // U+FEFF 不是空白 → 名字里带它仍然是一个占位符，名字本身是 `a` 加一个 U+FEFF 加 `b`。
    let named = placeholders("/users/{a\u{feff}b}");
    assert_eq!(named.len(), 1, "U+FEFF 不是空白，名字里带它仍算占位符");
    assert_eq!(named[0], "a\u{feff}b");
}
