//! OAuth2 配置里 `{{var}}` 的替换 —— 这条规则只有这一处实现。
//!
//! 两侧都不再自己做替换：渲染进程把**变量池**交给后端，MCP 把**变量池**交给
//! `ensure_token`，两边都由 core 在取 token 之前替换。规则只有这一份，故同一个
//! `{{var}}` 在 URL 里能解析、在 `tokenUrl` 里也能解析。
//!
//! **被替换的字段**只有四个：token 端点、授权端点、client id 与 scope —— 它们是会被
//! 发出去的地址与标识。
//!
//! **刻意不替换的字段**是密钥与一次性输入：`clientSecret` / `password` / `authCode`。
//! 密钥写成 `{{var}}` 很常见（环境变量表里放 secret），但解出来的值会顺着这份配置
//! 一路进 token 请求，而那份配置会被 MCP 的 `api_resolve_endpoint` 回显给 AI 客户端。
//! 少替换几个字段换来密钥不被解析层复制，是划算的 —— 要按环境给 secret 就写进环境本身。
//! 替换发生在 core 内部（而不是解析请求的那一层），密钥更不会进任何被回显的结构体。

use std::collections::HashMap;

use crate::domain::OAuth2Auth;
use crate::template::resolve_template;

/// 会被替换的字段（线上形状，camelCase）。
///
/// 渲染进程那侧有一份同名清单（`src/lib/http-build/auth.ts` 的
/// `OAUTH2_SUBSTITUTED_FIELDS`），两份清单由 `fixtures/oauth2-config-parity.json` 钉住。
pub const SUBSTITUTED_FIELDS: [&str; 4] = ["tokenUrl", "authorizationUrl", "clientId", "scope"];

/// 刻意**不**替换的字段（线上形状，camelCase）。
pub const KEPT_FIELDS: [&str; 3] = ["clientSecret", "password", "authCode"];

/// 一份 OAuth2 配置替换后的结果。
pub struct SubstitutedConfig {
    /// 替换后的配置。传入的那份不动。
    pub config: OAuth2Auth,
    /// 每个被替换的字段里没解析出来的变量名。字段名是线上形状（camelCase）。
    pub missing: Vec<(&'static str, Vec<String>)>,
}

/// 用 `vars` 替换 OAuth2 配置里那四个字段的 `{{var}}`。
///
/// 没解析出来的变量原样留在字段里（仍是 `{{name}}`），并进
/// [`SubstitutedConfig::missing`]。原样留着是为了让调用方报出「缺哪个变量」，
/// 而不是把 token 请求发到一个字面量地址上。
pub fn substitute_config(cfg: &OAuth2Auth, vars: &HashMap<String, String>) -> SubstitutedConfig {
    let token_url = resolve_template(&cfg.token_url, vars);
    let authorization_url = resolve_template(&cfg.authorization_url, vars);
    let client_id = resolve_template(&cfg.client_id, vars);
    let scope = resolve_template(&cfg.scope, vars);

    let mut missing: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (field, r) in [
        ("tokenUrl", &token_url),
        ("authorizationUrl", &authorization_url),
        ("clientId", &client_id),
        ("scope", &scope),
    ] {
        if !r.missing.is_empty() {
            missing.push((field, r.missing.clone()));
        }
    }

    SubstitutedConfig {
        config: OAuth2Auth {
            token_url: token_url.resolved,
            authorization_url: authorization_url.resolved,
            client_id: client_id.resolved,
            scope: scope.resolved,
            ..cfg.clone()
        },
        missing,
    }
}
