//! `{{var}}` 替换与缺变量清单的原语行为。

use std::collections::HashMap;

use super::{list_missing_vars, resolve_template};

// ---- resolve_template --------------------------------------------

#[test]
fn resolve_template_hits_known_var() {
    let mut vars = HashMap::new();
    vars.insert("name".into(), "alice".into());
    let r = resolve_template("hello {{name}}", &vars);
    assert_eq!(r.resolved, "hello alice");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_unknown_leaves_marker() {
    let vars = HashMap::new();
    let r = resolve_template("hi {{who}}", &vars);
    assert_eq!(r.resolved, "hi {{who}}");
    assert_eq!(r.missing, vec!["who"]);
}

#[test]
fn resolve_template_dedupes_missing_list() {
    let vars = HashMap::new();
    let r = resolve_template("{{x}}/{{x}}/{{y}}", &vars);
    assert_eq!(r.resolved, "{{x}}/{{x}}/{{y}}");
    assert_eq!(r.missing, vec!["x", "y"]);
}

#[test]
fn resolve_template_empty_input_returns_empty() {
    let vars = HashMap::new();
    let r = resolve_template("", &vars);
    assert_eq!(r.resolved, "");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_chinese_var_name_and_value() {
    let mut vars = HashMap::new();
    vars.insert("名字".into(), "小明".into());
    // `\w+` 即 `[A-Za-z0-9_]+`，非 ASCII 名字不匹配标记语法；带空白的同样不算。

    let r = resolve_template("hi {{名字}}", &vars);
    assert_eq!(r.resolved, "hi {{名字}}");
    let r = resolve_template("hi {{ name }}", &vars);
    assert_eq!(r.resolved, "hi {{ name }}");
}

#[test]
fn resolve_template_empty_value_is_kept() {
    // 空字符串条目算命中，替换产出空片段而不是留下标记。

    let mut vars = HashMap::new();
    vars.insert("empty".into(), "".into());
    let r = resolve_template("x={{empty}}!", &vars);
    assert_eq!(r.resolved, "x=!");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_doubled_marker_does_not_recurse() {
    // 替换结果里再出现的 `{{...}}` 不会被同一趟扫到：`a = "{{b}}"` 展开后仍是 `{{b}}`，
    // 且对已展开的串再跑一次结果不变。
    let mut vars = HashMap::new();
    vars.insert("a".into(), "{{b}}".into());
    let r = resolve_template("{{a}}", &vars);
    assert_eq!(r.resolved, "{{b}}");
    assert!(r.missing.is_empty(), "missing = {:?}", r.missing);

    // 叠写形状原样保留：第一个 `}}` 就截断，内层标记不算 word。
    let mut vars2 = HashMap::new();
    vars2.insert("a".into(), "X".into());
    let r2 = resolve_template("{{{{a}}}}", &vars2);
    assert_eq!(r2.resolved, "{{{{a}}}}");
    assert!(r2.missing.is_empty());
    assert!(!r2.resolved.contains("X"));
}

// ---- list_missing_vars -------------------------------------------

#[test]
fn list_missing_vars_dedupes_and_preserves_order() {
    let v = list_missing_vars("{{a}}-{{b}}-{{a}}/{{c}}");
    assert_eq!(v, vec!["a", "b", "c"]);
}

#[test]
fn list_missing_vars_ignores_non_word_markers() {
    let v = list_missing_vars("{{ with space }}{{ a.b }}{{ok}}");
    assert_eq!(v, vec!["ok"]);
}
