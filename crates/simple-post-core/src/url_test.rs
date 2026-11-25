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

/// 路径前缀改写 —— 与渲染进程的 `src/lib/url.ts` 同一套规则。
use super::{
    apply_rewrites, clean_rewrites, normalize_rewrite_from, try_apply_rewrite, validate_rewrites,
};
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
    let rules = [rule("/api/air", "/api/project-a/air")];
    assert_eq!(
        apply_rewrites("/api/air/xxxxx", &rules),
        "/api/project-a/air/xxxxx"
    );
}

/// 命中必须落在路径边界上：`/api/air` 不该命中 `/api/airplane`。
#[test]
fn a_prefix_must_end_on_a_path_boundary() {
    let rules = [rule("/api/air", "/api/project-a/air")];
    assert_eq!(
        apply_rewrites("/api/airplane/xxxxx", &rules),
        "/api/airplane/xxxxx"
    );
}

/// 起点是路径根时，整条路径换到目标底下。
#[test]
fn a_root_start_replaces_the_whole_path() {
    let rules = [rule("/", "/gateway")];
    assert_eq!(
        apply_rewrites("/api/air/list", &rules),
        "/gateway/api/air/list"
    );
    assert_eq!(apply_rewrites("/", &rules), "/gateway/");
}

/// 目标带 scheme 时不改写：改写只准改路径，不准把请求挪到另一个主机上去。
#[test]
fn an_absolute_target_is_refused() {
    let rules = [rule("/api/air", "https://other.example/api")];
    assert_eq!(apply_rewrites("/api/air/xxxxx", &rules), "/api/air/xxxxx");
    assert_eq!(try_apply_rewrite("/api/air/xxxxx", &rules[0]), None);
}

/// 端点自带完整地址时，改写只作用在它的路径上，主机与查询串一概不动。
#[test]
fn an_absolute_url_is_rewritten_in_its_path_only() {
    let rules = [rule("/api/air", "/api/project-a/air")];
    assert_eq!(
        apply_rewrites("https://h.example/api/air/list?x=1", &rules),
        "https://h.example/api/project-a/air/list?x=1"
    );
}

/// 没有起点就没有规则。
#[test]
fn a_rule_without_a_start_changes_nothing() {
    let rules = [rule("", "/api/project-a/air")];
    assert_eq!(apply_rewrites("/api/air/xxxxx", &rules), "/api/air/xxxxx");
}

/// 目标为空串等于去掉这一段前缀。
#[test]
fn an_empty_target_drops_the_prefix() {
    let rules = [rule("/api/air", "")];
    assert_eq!(apply_rewrites("/api/air/xxxxx", &rules), "/xxxxx");
}

/// 首条命中即停止：后续规则不再作用在已改写的路径上。
#[test]
fn the_first_matching_rule_wins_and_does_not_chain() {
    let rules = [
        rule("/api/air", "/v1/air"),
        rule("/api", "/v2"),
        rule("/v1/air", "/chained"),
    ];
    assert_eq!(apply_rewrites("/api/air/list", &rules), "/v1/air/list");
}

/// 首条不命中时回落到后续规则。
#[test]
fn a_miss_falls_through_to_the_next_rule() {
    let rules = [rule("/api/air", "/v1/air"), rule("/api", "/v2")];
    assert_eq!(apply_rewrites("/api/water/list", &rules), "/v2/water/list");
}

/// 规则顺序决定优先级：`/` 在前时全部路径都走它，交换后具体前缀优先。
#[test]
fn rule_order_decides_which_prefix_wins() {
    let catch_all_first = [rule("/", "/gateway"), rule("/api/air", "/v1/air")];
    assert_eq!(
        apply_rewrites("/api/air/list", &catch_all_first),
        "/gateway/api/air/list"
    );
    let specific_first = [rule("/api/air", "/v1/air"), rule("/", "/gateway")];
    assert_eq!(
        apply_rewrites("/api/air/list", &specific_first),
        "/v1/air/list"
    );
}

/// 空起点与非法目标在运行时被跳过，由下一条命中。
#[test]
fn an_unusable_rule_is_skipped() {
    let rules = [
        rule("", "/nowhere"),
        rule("/api/air", "//other.example/api"),
        rule("/api/air", "/v1/air"),
    ];
    assert_eq!(apply_rewrites("/api/air/list", &rules), "/v1/air/list");
}

/// 判重的规范化口径：去首尾空白、去末尾所有 `/`，全 `/` 归为 `/`。
#[test]
fn normalize_rewrite_from_strips_whitespace_and_trailing_slashes() {
    assert_eq!(normalize_rewrite_from("  /api/air/  "), "/api/air");
    assert_eq!(normalize_rewrite_from("/api/air//"), "/api/air");
    assert_eq!(normalize_rewrite_from("/"), "/");
    assert_eq!(normalize_rewrite_from("///"), "/");
    assert_eq!(normalize_rewrite_from("   "), "");
}

/// 重复起点（含末尾斜杠差异）被拒绝。
#[test]
fn duplicate_starts_are_rejected() {
    let err = validate_rewrites(&[rule("/api/air", "/v1"), rule("/api/air/", "/v2")])
        .expect_err("duplicate start must be rejected");
    assert!(
        matches!(err, crate::error::AppError::Validation(_)),
        "{err:?}"
    );
    let err = validate_rewrites(&[rule("/", "/v1"), rule("///", "/v2")])
        .expect_err("duplicate root start must be rejected");
    assert!(
        matches!(err, crate::error::AppError::Validation(_)),
        "{err:?}"
    );
}

/// 空起点与绝对目标各自被拒绝。
#[test]
fn empty_start_and_absolute_target_are_rejected() {
    assert!(validate_rewrites(&[rule("  ", "/v1")]).is_err());
    assert!(validate_rewrites(&[rule("/api/air", "https://other.example/api")]).is_err());
    assert!(validate_rewrites(&[rule("/api/air", "//other.example/api")]).is_err());
    assert!(validate_rewrites(&[rule("/api/air", "/v1"), rule("/api/water", "/v2")]).is_ok());
}

/// 全空白占位行被剔除，其余规则的原样与顺序保留。
#[test]
fn clean_rewrites_drops_only_blank_placeholders() {
    let rules = [
        rule(" ", "  "),
        rule("/api/air", "/v1"),
        rule("/api/air", ""),
        rule("", ""),
    ];
    let cleaned = clean_rewrites(&rules);
    assert_eq!(cleaned, vec![rule("/api/air", "/v1"), rule("/api/air", "")]);
}
