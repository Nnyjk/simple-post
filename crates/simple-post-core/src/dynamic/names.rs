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
    /// 端点的 path。**必须是改写之后的那一份**：路径前缀改写会换掉其中的一段，
    /// 改写没命中时就是原路径。扫描的是这次请求真正会发出去的那一份——改写目标里
    /// 的标记算数，没被改写掉的那段里的标记不算数。
    pub url: &'a str,
    pub params: &'a [KeyValue],
    /// URL 路径里的占位符与它们各自的值。它们替换进路径本身。
    ///
    /// 值里的 `{{var}}` 照样会被替换，故它们里的标记同样算被引用——不扫的话，只在路径参数值里
    /// 出现的动态变量永远不会被取数。
    pub path_params: &'a [KeyValue],
    pub headers: &'a [KeyValue],
    pub body: &'a RequestBody,
    pub auth: &'a AuthConfig,
}

/// 模板里出现的全部 `{{name}}` 名字，按首次出现排序。
///
/// 只扫**会被替换**的字段：base URL、端点那一份**生效后**的 path、启用的且 key 非空的
/// 参数值 / 路径参数值 / header 值、body 内容、以及鉴权里那三种方案（bearer / basic /
/// apikey）。OAuth2 的配置字段不被变量替换，扫了会让一条根本没被用到的定义去取数。
///
/// 路径前缀改写已经由调用方算进 `url` 了：命中时扫改写后的路径，没命中时扫原路径。
/// 扫描器自己不知道改写规则——它只认这次请求真正会发出去的那一份路径。
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
