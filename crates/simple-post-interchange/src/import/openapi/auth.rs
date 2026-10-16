//! `security` → [`AuthConfig`]：operation 级覆盖文档级，映射不了的方案出声丢弃。

use serde_json::{Map, Value};
use simple_post_core::domain::{ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth};

use crate::diagnostics::Diagnostic;
use crate::spec::escape_pointer_token;

use super::refs::deref;
use super::{text, Ctx, DocView};

/// operation 级 `security` 覆盖文档级。空数组是显式的「不要鉴权」。
pub(super) fn auth_of(
    ctx: &mut Ctx,
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
) -> AuthConfig {
    let at = format!("{pointer}/security");
    let source = operation
        .get("security")
        .or_else(|| view.root.get("security"));
    let Some(Value::Array(requirements)) = source else {
        return AuthConfig::default();
    };
    let Some(Value::Object(first)) = requirements.first() else {
        return AuthConfig::default();
    };
    for name in first.keys() {
        if let Some(auth) = scheme_of(ctx, view, name, &at) {
            return auth;
        }
    }
    AuthConfig::default()
}

/// 一条 securityScheme → [`AuthConfig`]。映射不了返回 `None`，诊断已报在里面。
fn scheme_of(ctx: &mut Ctx, view: &DocView, name: &str, pointer: &str) -> Option<AuthConfig> {
    let at = format!("{pointer}/{}", escape_pointer_token(name));
    let raw = view
        .root
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .and_then(|s| s.get(name))?;
    let scheme = deref(ctx, view.root, raw, &at)?;
    let Some(scheme) = scheme.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(&at, "securityScheme 不是对象，已跳过"));
        return None;
    };

    match text(scheme, "type").unwrap_or_default().as_str() {
        "http" => {
            let http = text(scheme, "scheme")
                .unwrap_or_default()
                .to_ascii_lowercase();
            match http.as_str() {
                // 外来文档给不出 token。空串表示「用 bearer，token 待填」：
                // 发送链路因此不会加 Authorization 头，并把它报成缺失变量。
                "bearer" => Some(AuthConfig {
                    kind: AuthType::Bearer,
                    bearer: Some(String::new()),
                    ..Default::default()
                }),
                "basic" => Some(AuthConfig {
                    kind: AuthType::Basic,
                    basic: Some(BasicAuth::default()),
                    ..Default::default()
                }),
                other => {
                    ctx.diagnostics.push(
                        Diagnostic::warn(&at, "http 认证方案不是 bearer/basic，无法导入")
                            .dropping(format!("http {other}")),
                    );
                    None
                }
            }
        }
        "apiKey" => {
            let key = text(scheme, "name").unwrap_or_default();
            let location = match text(scheme, "in").unwrap_or_default().as_str() {
                "header" => ApiKeyLocation::Header,
                "query" => ApiKeyLocation::Query,
                other => {
                    ctx.diagnostics.push(
                        Diagnostic::warn(
                            &at,
                            "ApiKeyLocation 只有 header 与 query，这个 apiKey 无法导入",
                        )
                        .dropping(format!("apiKey in: {other}")),
                    );
                    return None;
                }
            };
            Some(AuthConfig {
                kind: AuthType::Apikey,
                apikey: Some(ApiKeyAuth {
                    key,
                    value: String::new(),
                    in_location: location,
                }),
                ..Default::default()
            })
        }
        other => {
            ctx.diagnostics.push(
                Diagnostic::warn(&at, "OpenID Connect 与 oauth2 的配置结构对不上，无法导入")
                    .dropping(other.to_string()),
            );
            None
        }
    }
}
