//! `version.rs` 的规则逐条钉住。全部是纯函数，不碰网络、不碰环境变量。

use std::cmp::Ordering;

use super::*;

#[test]
fn numeric_segments_beat_string_order() {
    // 字符串比较会说 "0.1.9" > "0.1.10"，逐段按数字比才对。
    assert_eq!(compare("0.1.9", "0.1.10"), Ok(Ordering::Less));
    assert!(is_newer("0.1.10", "0.1.9").unwrap());
}

#[test]
fn leading_v_is_ignored() {
    assert_eq!(compare("v0.1.34", "0.1.34"), Ok(Ordering::Equal));
    assert_eq!(parse("v1.2.3"), parse("1.2.3"));
}

#[test]
fn missing_segments_count_as_zero() {
    assert_eq!(compare("1.0", "1.0.0"), Ok(Ordering::Equal));
    assert_eq!(compare("1.0.0.1", "1.0"), Ok(Ordering::Greater));
}

#[test]
fn prerelease_suffix_matches_the_release() {
    // 注释里写明的取舍：后缀不参与比较，故 rc 与正式版同版。
    assert_eq!(compare("0.1.34-rc1", "0.1.34"), Ok(Ordering::Equal));
}

#[test]
fn minor_bump_beats_patch_bump() {
    assert_eq!(compare("0.2.0", "0.1.99"), Ok(Ordering::Greater));
}

#[test]
fn non_numeric_segments_are_rejected() {
    assert_eq!(parse("x.y"), Err(BadVersion));
    assert_eq!(compare("x.y", "0.1.0"), Err(BadVersion));
    assert_eq!(parse(""), Err(BadVersion));
    assert_eq!(parse("1..0"), Err(BadVersion));
    assert_eq!(parse("v"), Err(BadVersion));
}

#[test]
fn is_newer_is_false_for_equal_and_older() {
    assert!(!is_newer("0.1.34", "0.1.34").unwrap());
    assert!(!is_newer("v0.1.34", "0.1.34").unwrap());
    assert!(!is_newer("0.1.33", "0.1.34").unwrap());
    assert!(is_newer("0.1.35", "0.1.34").unwrap());
}
