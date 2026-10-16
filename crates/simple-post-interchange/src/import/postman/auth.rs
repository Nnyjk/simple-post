//! `auth.type` 的降级映射：只读选中方案的那一份载荷。

use serde_json::{Map, Value};
use simple_post_core::domain::{ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth};

use crate::diagnostics::Diagnostic;

use super::{field_text, Ctx};

pub(super) fn parse_auth(req: &Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> AuthConfig {
    let Some(auth) = req.get("auth") else {
        return AuthConfig::default();
    };
    let Some(a) = auth.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(ptr, "auth 不是 JSON 对象，已按无 auth 处理").dropping("auth"));
        return AuthConfig::default();
    };
    let scheme = field_text(a, "type").unwrap_or_default();

    match scheme.as_str() {
        "noauth" | "" => AuthConfig::default(),
        "basic" => {
            let p = auth_params(a, "basic");
            AuthConfig {
                kind: AuthType::Basic,
                basic: Some(BasicAuth {
                    username: auth_param(&p, "username"),
                    password: auth_param(&p, "password"),
                }),
                ..Default::default()
            }
        }
        "bearer" => {
            let p = auth_params(a, "bearer");
            AuthConfig {
                kind: AuthType::Bearer,
                bearer: Some(auth_param(&p, "token")),
                ..Default::default()
            }
        }
        "apikey" => {
            let p = auth_params(a, "apikey");
            let location = match auth_param(&p, "in").as_str() {
                "query" => ApiKeyLocation::Query,
                "header" | "" => ApiKeyLocation::Header,
                other => {
                    // 只有 header / query 两种位置。猜成 header 会把凭据发到错的地方，
                    // 故整段丢弃并出声。
                    ctx.diagnostics.push(
                        Diagnostic::warn(ptr, "apikey 的位置无法表达，整段 auth 已丢弃")
                            .dropping(format!("apikey auth（in: {other}）")),
                    );
                    return AuthConfig::default();
                }
            };
            AuthConfig {
                kind: AuthType::Apikey,
                apikey: Some(ApiKeyAuth {
                    key: auth_param(&p, "key"),
                    value: auth_param(&p, "value"),
                    in_location: location,
                }),
                ..Default::default()
            }
        }
        other => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "未知的 auth 方案，已按无 auth 处理").dropping(other));
            AuthConfig::default()
        }
    }
}

/// Postman 的 auth 参数是 `[{key, value}]`；少数导出把它写成单个对象，两种都收。
fn auth_params(a: &Map<String, Value>, scheme: &str) -> Vec<(String, String)> {
    let read = |o: &Map<String, Value>| {
        (
            field_text(o, "key").unwrap_or_default(),
            field_text(o, "value").unwrap_or_default(),
        )
    };
    let mut out = Vec::new();
    match a.get(scheme) {
        Some(Value::Array(arr)) => {
            for e in arr.iter().filter_map(Value::as_object) {
                out.push(read(e));
            }
        }
        Some(Value::Object(o)) => out.push(read(o)),
        _ => {}
    }
    out
}

fn auth_param(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}
