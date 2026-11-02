//! 请求引用了哪些 `{{name}}`。
//!
//! 扫描范围与替换范围必须一致：扫到的名字会被取数，没扫到的名字不会被取数。

use std::collections::HashSet;

use crate::domain::{AuthConfig, AuthType, KeyValue, RequestBody};
use crate::template::list_missing_vars;

/// 请求模板的各段。扫描「哪些 `{{name}}` 被引用了」只需要这些。
pub struct TemplateParts<'a> {
    /// base URL。它可能自身带变量，故与 path 一起扫。
    pub base_url: &'a str,
    /// 那一格的实际访问地址。它会被 `{{var}}` 替换，故它里面的标记同样算被引用 ——
    /// 不扫的话，只在代理地址里出现的动态变量永远不会被取数，替换后仍是字面量 `{{name}}`。
    pub proxy: Option<&'a str>,
    /// 端点的 path。
    pub url: &'a str,
    pub params: &'a [KeyValue],
    /// URL 路径里的占位符与它们各自的值。它们替换进路径本身。
    ///
    /// 值里的 `{{var}}` 照样会被替换（见 `variable_fill::resolve_endpoint`），故它们
    /// 里的标记同样算被引用 —— 不扫的话，只在路径参数值里出现的动态变量永远不会被
    /// 取数，替换后仍是字面量 `{{name}}`。
    pub path_params: &'a [KeyValue],
    pub headers: &'a [KeyValue],
    pub body: &'a RequestBody,
    pub auth: &'a AuthConfig,
}

/// 模板里出现的全部 `{{name}}` 名字，按首次出现排序。
///
/// 只扫**会被替换**的字段：base URL、那一格的代理地址、启用的且 key 非空的参数值、
/// 启用的且 key 非空的路径参数值、启用的且 key 非空的 header 值、body 内容、以及鉴权里
/// 那三种方案（bearer / basic / apikey）。OAuth2 的配置字段不被变量替换，故不在扫描
/// 范围内——扫了会让一条根本没被用到的定义去取数。
///
/// 未启用的参数与 header 里的变量不算被引用：它们不会发出，其中的变量不是这次请求的问题。
pub fn list_names(parts: &TemplateParts<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |text: &str| {
        for name in list_missing_vars(text) {
            if seen.insert(name.clone()) {
                out.push(name);
            }
        }
    };

    add(parts.base_url);
    // 代理地址也会被 `{{var}}` 替换，故它里面的标记同样算被引用：没有代理、代理是空串
    // 时不扫。
    if let Some(proxy) = parts.proxy.filter(|p| !p.is_empty()) {
        add(proxy);
    }
    add(parts.url);
    for p in parts.params {
        if p.enabled && !p.key.is_empty() {
            add(&p.value);
        }
    }
    for p in parts.path_params {
        if p.enabled && !p.key.is_empty() {
            add(&p.value);
        }
    }
    for h in parts.headers {
        if h.enabled && !h.key.is_empty() {
            add(&h.value);
        }
    }
    if !parts.body.content.is_empty() {
        add(&parts.body.content);
    }
    match parts.auth.kind {
        AuthType::Bearer => {
            if let Some(bearer) = &parts.auth.bearer {
                add(bearer);
            }
        }
        AuthType::Basic => {
            if let Some(basic) = &parts.auth.basic {
                add(&basic.username);
                add(&basic.password);
            }
        }
        AuthType::Apikey => {
            if let Some(apikey) = &parts.auth.apikey {
                add(&apikey.key);
                add(&apikey.value);
            }
        }
        // OAuth2 的配置字段不参与变量替换，故不扫；`None` 也不带任何值。
        AuthType::None | AuthType::Oauth2 => {}
    }
    out
}
