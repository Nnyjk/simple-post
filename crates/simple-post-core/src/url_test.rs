//! 百分号编码的字符集与 JS 的 `encodeURIComponent` 对齐，以及路径前缀改写。

use super::percent_encode;

/// `encodeURIComponent` 不转义的那批字符必须原样保留。
#[test]
fn the_kept_character_set_is_the_one_encodeuricomponent_keeps() {
    // `encodeURIComponent` 不转义 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`。
    for c in "abcXYZ019-_.!~*'()".chars() {
        let s = c.to_string();
        assert_eq!(percent_encode(&s), s, "{c:?} 必须原样保留");
    }
}

/// 其余字符一律 `%XX`，十六进制大写。
#[test]
fn everything_else_is_percent_encoded_with_uppercase_hex() {
    assert_eq!(percent_encode("a b"), "a%20b");
    assert_eq!(percent_encode("a+b"), "a%2Bb");
    assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(percent_encode("a/b"), "a%2Fb");
    assert_eq!(percent_encode("%"), "%25");
}

/// 非 ASCII 字符按 UTF-8 逐字节编码。
#[test]
fn non_ascii_is_encoded_byte_by_byte_as_utf8() {
    assert_eq!(percent_encode("中"), "%E4%B8%AD");
    assert_eq!(percent_encode("é"), "%C3%A9");
}

/// 路径前缀改写 —— 与渲染进程的 `src/lib/url.ts::applyRewrite` 同一套规则。
use super::apply_rewrite;
use crate::domain::RewriteRule;

fn rule(from: &str, to: &str) -> RewriteRule {
    RewriteRule {
        from: from.to_string(),
        to: to.to_string(),
    }
}

/// 前缀命中时换成目标，其余部分原样跟着走。
#[test]
fn a_matching_prefix_is_replaced_by_the_target() {
    let r = rule("/api/air", "/api/project-a/air");
    assert_eq!(
        apply_rewrite("/api/air/xxxxx", Some(&r)),
        "/api/project-a/air/xxxxx"
    );
}

/// 命中必须落在路径边界上：`/api/air` 不该命中 `/api/airplane`。
#[test]
fn a_prefix_must_end_on_a_path_boundary() {
    let r = rule("/api/air", "/api/project-a/air");
    assert_eq!(
        apply_rewrite("/api/airplane/xxxxx", Some(&r)),
        "/api/airplane/xxxxx"
    );
}

/// 起点是路径根时，整条路径换到目标底下。
#[test]
fn a_root_start_replaces_the_whole_path() {
    let r = rule("/", "/gateway");
    assert_eq!(
        apply_rewrite("/api/air/list", Some(&r)),
        "/gateway/api/air/list"
    );
    assert_eq!(apply_rewrite("/", Some(&r)), "/gateway/");
}

/// 目标带 scheme 时不改写：改写只准改路径，不准把请求挪到另一个主机上去。
#[test]
fn an_absolute_target_is_refused() {
    let r = rule("/api/air", "https://other.example/api");
    assert_eq!(apply_rewrite("/api/air/xxxxx", Some(&r)), "/api/air/xxxxx");
}

/// 端点自带完整地址时，改写只作用在它的路径上，主机与查询串一概不动。
#[test]
fn an_absolute_url_is_rewritten_in_its_path_only() {
    let r = rule("/api/air", "/api/project-a/air");
    assert_eq!(
        apply_rewrite("https://h.example/api/air/list?x=1", Some(&r)),
        "https://h.example/api/project-a/air/list?x=1"
    );
}

/// 没有起点就没有规则。
#[test]
fn a_rule_without_a_start_changes_nothing() {
    let r = rule("", "/api/project-a/air");
    assert_eq!(apply_rewrite("/api/air/xxxxx", Some(&r)), "/api/air/xxxxx");
}

/// 目标为空串等于去掉这一段前缀。
#[test]
fn an_empty_target_drops_the_prefix() {
    let r = rule("/api/air", "");
    assert_eq!(apply_rewrite("/api/air/xxxxx", Some(&r)), "/xxxxx");
}
