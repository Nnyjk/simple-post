//! 扫描的是这次请求**真正会发出去**的那一份路径。
//!
//! 路径前缀改写由调用方算进 `url`：命中时 `url` 是改写后的路径，没命中时是原路径。
//! 扫描器自己不知道改写规则——它只认生效的那一份，故改写目标里的标记算数，没被改写掉
//! 的那一段里的标记不算数。

use super::{list_names, TemplateParts};
use crate::domain::{AuthConfig, RequestBody};

/// 一份只带地址的模板：其余各段都是空的，故扫出来的名字只可能来自那两段地址。
fn parts<'a>(
    base_url: &'a str,
    url: &'a str,
    body: &'a RequestBody,
    auth: &'a AuthConfig,
) -> TemplateParts<'a> {
    TemplateParts {
        base_url,
        url,
        params: &[],
        path_params: &[],
        headers: &[],
        body,
        auth,
    }
}

/// 生效后的路径里的标记算被引用：那条路径会进 URL，标记会被替换。
#[test]
fn the_effective_path_is_scanned_for_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts(
        "https://api.example.com",
        "/api/{{token}}/air",
        &body,
        &auth,
    );
    assert_eq!(
        list_names(&parts),
        vec!["token".to_string()],
        "生效后的路径会被替换，故它里面的名字算被引用"
    );
}

/// 路径里没有标记时，扫出来的名字只有 base URL 里的那些。
#[test]
fn a_path_without_markers_leaves_the_names_untouched() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts("https://{{host}}/api", "/x", &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string()],
        "路径里没有标记时，扫出来的名字只有 base URL 里的那些"
    );
}

/// 两处都有的名字只出现一次。
#[test]
fn a_name_in_the_base_url_and_in_the_path_appears_once() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts(
        "https://{{host}}/api",
        "/api/{{host}}/{{token}}",
        &body,
        &auth,
    );
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两处都有的名字只出现一次，路径独有的名字也要扫出来"
    );
}

/// 两段地址各自的名字都要扫出来。
#[test]
fn the_base_url_and_the_path_contribute_their_own_names() {
    let body = RequestBody::default();
    let auth = AuthConfig::default();
    let parts = parts("https://{{host}}/api", "/api/{{token}}", &body, &auth);
    assert_eq!(
        list_names(&parts),
        vec!["host".to_string(), "token".to_string()],
        "两段地址各自的名字都要扫出来"
    );
}
