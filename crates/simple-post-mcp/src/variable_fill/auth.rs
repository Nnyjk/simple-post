//! 鉴权配置里各字段的 `{{var}}` 替换。
//!
//! 返回**新的** `AuthConfig`，不改传入值；鉴权文本里缺的变量汇入调用方的 missing 列表。
//!
//! **OAuth2 的载荷不在这里替换。** 那条规则（哪些字段被替换、哪些密钥不替换）住在 core
//! 的 `simple_post_core::oauth2`（见 `substitute_config`），由 core 在取 token 之前
//! 替换。MCP 侧把变量池交给 `ensure_token` 即可，不必在这里重写一遍规则 —— 两边各写
//! 一份时，渲染进程与 MCP 就会对同一个 `{{var}}` 给出两种结果。

use std::collections::HashMap;

use simple_post_core::domain::{ApiKeyAuth, AuthConfig, AuthType, BasicAuth};

use super::template::resolve_template;
// -------------------------------------------------------------------------
// 鉴权 —— 对 bearer / basic / apikey 各字段做 `{{var}}` 替换
// -------------------------------------------------------------------------

/// 对鉴权配置里所有带用户可见文本的字段做 `{{var}}` 替换。返回**新的**
/// `AuthConfig`，不改传入值（调用方可能在多次发送之间复用同一个端点对象）。
/// 鉴权文本里缺的变量汇入 URL 扫描产出的同一份 missing 列表。
///
/// OAuth2 的载荷原样带过去：它的替换发生在取 token 之前，由 core 做。
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
    (
        AuthConfig {
            kind,
            bearer,
            basic,
            apikey,
            // OAuth2 的载荷原样带着：替换发生在取 token 之前，由 core 做。
            oauth2: auth.oauth2.clone(),
        },
        missing,
    )
}
