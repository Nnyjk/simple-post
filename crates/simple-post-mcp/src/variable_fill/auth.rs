//! 鉴权配置里各字段的 `{{var}}` 替换。
//!
//! 返回**新的** `AuthConfig`，不改传入值；鉴权文本里缺的变量汇入调用方的 missing 列表。

use std::collections::HashMap;

use simple_post_core::domain::{ApiKeyAuth, AuthConfig, AuthType, BasicAuth, OAuth2Auth};

use super::template::resolve_template;
// -------------------------------------------------------------------------
// 鉴权 —— 对 bearer / basic / apikey 各字段做 `{{var}}` 替换
// -------------------------------------------------------------------------

/// 对鉴权配置里所有带用户可见文本的字段做 `{{var}}` 替换。返回**新的**
/// `AuthConfig`，不改传入值（调用方可能在多次发送之间复用同一个端点对象）。
/// 鉴权文本里缺的变量汇入 URL 扫描产出的同一份 missing 列表。
pub(super) fn resolve_auth(
    auth: &AuthConfig,
    vars: &HashMap<String, String>,
) -> (AuthConfig, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record = |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            missing.push(var);
        }
    };

    let kind = auth.kind.clone();
    let bearer = match (kind.clone(), auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) => {
            let r = resolve_template(b, vars);
            for m in r.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(r.resolved)
        }
        _ => auth.bearer.clone(),
    };
    let basic = match (kind.clone(), auth.basic.as_ref()) {
        (AuthType::Basic, Some(b)) => {
            let u = resolve_template(&b.username, vars);
            for m in u.missing {
                record(&mut missing, &mut seen, m);
            }
            let p = resolve_template(&b.password, vars);
            for m in p.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(BasicAuth {
                username: u.resolved,
                password: p.resolved,
            })
        }
        _ => auth.basic.clone(),
    };
    let apikey = match (kind.clone(), auth.apikey.as_ref()) {
        (AuthType::Apikey, Some(a)) => {
            let k = resolve_template(&a.key, vars);
            for m in k.missing {
                record(&mut missing, &mut seen, m);
            }
            let v = resolve_template(&a.value, vars);
            for m in v.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(ApiKeyAuth {
                key: k.resolved,
                value: v.resolved,
                in_location: a.in_location,
            })
        }
        _ => auth.apikey.clone(),
    };
    let oauth2 = match (kind.clone(), auth.oauth2.as_ref()) {
        (AuthType::Oauth2, Some(o)) => Some(resolve_oauth2(o, vars, &mut missing, &mut seen)),
        _ => auth.oauth2.clone(),
    };
    (
        AuthConfig {
            kind,
            bearer,
            basic,
            apikey,
            oauth2,
        },
        missing,
    )
}

/// OAuth2 配置里的 `{{var}}` 替换。
///
/// **只替换 token URL / authorization URL / client id / scope 这类会被发出去的地址与
/// 标识**，刻意不碰 `client_secret`、`password`、`auth_code`：密钥写成 `{{var}}` 很常见
/// （环境变量表里放 secret），但解出来的值会顺着本函数的返回值一路进
/// `ResolvedEndpoint.auth`，而那是被 MCP 的响应回显读到的。少替换几个字段换来密钥
/// 不被解析层复制，是划算的——要按环境给 secret 就写进环境本身。
fn resolve_oauth2(
    o: &OAuth2Auth,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
    seen: &mut HashMap<String, ()>,
) -> OAuth2Auth {
    let resolve_text = |text: &str, missing: &mut Vec<String>, seen: &mut HashMap<String, ()>| {
        let r = resolve_template(text, vars);
        for m in r.missing {
            if seen.insert(m.clone(), ()).is_none() {
                missing.push(m);
            }
        }
        r.resolved
    };
    let mut out = o.clone();
    out.token_url = resolve_text(&o.token_url, missing, seen);
    out.authorization_url = resolve_text(&o.authorization_url, missing, seen);
    out.client_id = resolve_text(&o.client_id, missing, seen);
    out.scope = resolve_text(&o.scope, missing, seen);
    out
}
