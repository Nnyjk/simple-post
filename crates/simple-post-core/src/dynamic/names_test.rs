//! 路径改写规则的目标与 base URL 一样会被 `{{var}}` 替换，故目标里的标记同样算被引用。

use super::{list_names, TemplateParts};
use crate::domain::{AuthConfig, RequestBody, RewriteRule};

/// 一份只带地址的模板：其余各段都是空的，故扫出来的名字只可能来自那两段地址。
fn parts<'a>(
    base_url: &'a str,
    rewrite: Option<&'a RewriteRule>,
    body: &'a RequestBody,
    auth: &'a AuthConfig,
) -> TemplateParts<'a> {
    TemplateParts {
        base_url,
        rewrite,
        url: "/x",
        params: &[],
        path_params: &[],
        headers: &[],
        body,
        auth,
    }
}

/// 一条改写规则：起点与目标。
fn rule(from: &str, to: &str) -> RewriteRule {
    RewriteRule {
        from: from.into(),
        to: to.into(),
    }
}

#[test]
fn the_rewrite_target_is_scanned_for_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let rule = rule("/api/air", "/api/{{token}}/air");
    let parts = parts("https://api.example.com", Some(&rule), &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["token".to_string()],
        "改写目标会被替换，故它里面的名字算被引用"
    );
}

#[test]
fn the_rewrite_from_is_not_scanned_for_names() {
    // 起点按原文匹配，不做变量替换，故它里面的标记永远不会被展开，也就不算被引用。
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let rule = rule("/api/{{token}}", "/api/air");
    let parts = parts("https://api.example.com", Some(&rule), &body, &auth);
    assert_eq!(
        list_names(&parts),
        Vec::<String>::new(),
        "起点里的标记不会被替换，不算被引用"
    );
}

#[test]
fn no_rewrite_leaves_the_names_untouched() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts("https://{{host}}/api", None, &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string()],
        "没有改写规则时，扫出来的名字只有 base URL 里的那些"
    );
}

#[test]
fn an_empty_rewrite_target_leaves_the_names_untouched() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let rule = rule("/api/air", "");
    let parts = parts("https://{{host}}/api", Some(&rule), &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string()],
        "目标是空串时，它里面没有标记可扫"
    );
}

#[test]
fn a_name_in_the_base_url_and_in_the_rewrite_target_appears_once() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let rule = rule("/api/air", "/api/{{host}}/{{token}}");
    let parts = parts("https://{{host}}/api", Some(&rule), &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两处都有的名字只出现一次，改写目标独有的名字也要扫出来"
    );
}

#[test]
fn the_base_url_and_the_rewrite_target_contribute_their_own_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let rule = rule("/api/air", "/api/{{token}}");
    let parts = parts("https://{{host}}/api", Some(&rule), &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两段地址各自的名字都要扫出来"
    );
}
