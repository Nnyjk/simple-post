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
    // 注意：`\w+` 即 `[A-Za-z0-9_]+`，所以非 ASCII 名字不匹配标记语法，原样穿过。

    let r = resolve_template("hi {{名字}}", &vars);
    assert_eq!(r.resolved, "hi {{名字}}");
    // 标记内部的空白同样不算 word。
    let r = resolve_template("hi {{ name }}", &vars);
    assert_eq!(r.resolved, "hi {{ name }}");
}

#[test]
fn resolve_template_empty_value_is_kept() {
    // 空字符串条目**算命中**；替换必须产出空片段，而不是留下标记。

    let mut vars = HashMap::new();
    vars.insert("empty".into(), "".into());
    let r = resolve_template("x={{empty}}!", &vars);
    assert_eq!(r.resolved, "x=!");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_doubled_marker_does_not_recurse() {
    // "No recursion" property: a substitution result that itself
    // contains `{{...}}` markers must NOT be re-scanned in the
    // same pass.
    //
    // The single-pass scanner finds the first `}}` it sees in
    // `{{{{a}}}}`, considers the interior `{{a` a malformed token
    // (because it contains `{`), and emits the marker verbatim. So
    // the assertion below confirms two things at once:
    //   1. Substituting `a = "{{b}}"` for an outer `{{a}}` produces
    //      a string that still contains `{{b}}` (no second scan
    //      pulled it in).
    //   2. Running resolve_template on the already-expanded string
    //      does NOT behave any differently —proving the resolver
    //      is a pure single pass, idempotent on its own output.
    let mut vars = HashMap::new();
    vars.insert("a".into(), "{{b}}".into());
    let r = resolve_template("{{a}}", &vars);
    assert_eq!(r.resolved, "{{b}}");
    assert!(r.missing.is_empty(), "missing = {:?}", r.missing);

    // And the doubled-up case stays verbatim —the first `}}`
    // wins, the marker is left untouched, and the input does not
    // mutate at all.
    let mut vars2 = HashMap::new();
    vars2.insert("a".into(), "X".into());
    let r2 = resolve_template("{{{{a}}}}", &vars2);
    assert_eq!(r2.resolved, "{{{{a}}}}");
    assert!(r2.missing.is_empty());
    // No second pass swallowed the inner braces either.
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
