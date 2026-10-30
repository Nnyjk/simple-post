//! `names.rs` 的测试：请求引用了哪些 `{{name}}`。
//!
//! 代理地址与 base URL 一样会被 `{{var}}` 替换，故代理地址里的标记同样算被引用。

use super::{list_names, TemplateParts};
use crate::domain::{AuthConfig, RequestBody};

/// 一份只带地址的模板：其余各段都是空的，故扫出来的名字只可能来自那两段地址。
fn parts<'a>(
    base_url: &'a str,
    proxy: Option<&'a str>,
    body: &'a RequestBody,
    auth: &'a AuthConfig,
) -> TemplateParts<'a> {
    TemplateParts {
        base_url,
        proxy,
        url: "/x",
        params: &[],
        path_params: &[],
        headers: &[],
        body,
        auth,
    }
}

#[test]
fn the_proxy_address_is_scanned_for_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts(
        "https://api.example.com",
        Some("https://gw.example.com/{{token}}"),
        &body,
        &auth,
    );
    assert_eq!(
        list_names(&parts),
        vec!["token".to_string()],
        "代理地址会被替换，故它里面的名字算被引用"
    );
}

#[test]
fn no_proxy_leaves_the_names_untouched() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts("https://{{host}}/api", None, &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string()],
        "没有代理时，扫出来的名字只有 base URL 里的那些"
    );
}

#[test]
fn an_empty_proxy_leaves_the_names_untouched() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts("https://{{host}}/api", Some(""), &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string()],
        "代理是空串时，它里面没有标记可扫"
    );
}

#[test]
fn a_name_in_the_base_url_and_in_the_proxy_appears_once() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts(
        "https://{{host}}/api",
        Some("https://gw.example.com/{{host}}/{{token}}"),
        &body,
        &auth,
    );
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两个地址里都有的名字只出现一次，代理独有的名字也要扫出来"
    );
}

#[test]
fn the_base_url_and_the_proxy_contribute_their_own_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts(
        "https://{{host}}/api",
        Some("https://gw.example.com/{{token}}"),
        &body,
        &auth,
    );
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两段地址各自的名字都要扫出来"
    );
}
