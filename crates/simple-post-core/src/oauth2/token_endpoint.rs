//! 与 token 端点的那一次往返：拼 form、发请求、解包应答。

use crate::domain::{OAuth2Auth, OAuth2ClientAuth, OAuth2GrantType};
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput};

use super::form::{encode_form, form_encode};
use super::parse::parse_token_response;
use super::OAuth2Token;
use super::{TOKEN_MAX_RESPONSE_KB, TOKEN_TIMEOUT_MS};

/// 按当前流程拼出一次完整取 token 的 form 字段。
pub(super) fn build_grant_form(cfg: &OAuth2Auth) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = vec![(
        "grant_type".to_string(),
        match cfg.grant_type {
            OAuth2GrantType::AuthorizationCode => "authorization_code",
            OAuth2GrantType::ClientCredentials => "client_credentials",
            OAuth2GrantType::Password => "password",
            // 到不了这里：`implicit` 没有 token 端点，`validate` 先拦下。
            OAuth2GrantType::Implicit => "implicit",
        }
        .to_string(),
    )];

    if !cfg.scope.trim().is_empty() {
        form.push(("scope".into(), cfg.scope.trim().to_string()));
    }
    match cfg.grant_type {
        OAuth2GrantType::AuthorizationCode => {
            form.push(("code".into(), super::parse::extract_code(&cfg.auth_code)));
            if !cfg.redirect_uri.trim().is_empty() {
                form.push(("redirect_uri".into(), cfg.redirect_uri.trim().to_string()));
            }
            if !cfg.code_verifier.trim().is_empty() {
                form.push(("code_verifier".into(), cfg.code_verifier.trim().to_string()));
            }
        }
        OAuth2GrantType::Password => {
            form.push(("username".into(), cfg.username.clone()));
            form.push(("password".into(), cfg.password.clone()));
        }
        _ => {}
    }
    append_extra_params(&mut form, cfg);
    append_client_credentials(&mut form, cfg);
    form
}

/// 用 `refresh_token` 换新的 access token（RFC 6749 §6）。
fn build_refresh_form(cfg: &OAuth2Auth, refresh_token: &str) -> Vec<(String, String)> {
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
    ];
    if !cfg.scope.trim().is_empty() {
        form.push(("scope".into(), cfg.scope.trim().to_string()));
    }
    append_extra_params(&mut form, cfg);
    // 刷新时 `client_auth` 仍是同一份凭据，且按 §6 建议收窄 scope 时可省略——
    // 这里沿用配置里的 scope，用户想沿用服务端默认值就把字段清空。
    append_client_credentials(&mut form, cfg);
    form
}

/// 客户端凭据：放在 Basic 头里就不进 form（RFC 6749 §2.3.1）。
fn append_client_credentials(form: &mut Vec<(String, String)>, cfg: &OAuth2Auth) {
    if cfg.client_auth != OAuth2ClientAuth::Body {
        return;
    }
    form.push(("client_id".into(), cfg.client_id.clone()));
    if !cfg.client_secret.is_empty() {
        form.push(("client_secret".into(), cfg.client_secret.clone()));
    }
}

fn append_extra_params(form: &mut Vec<(String, String)>, cfg: &OAuth2Auth) {
    for kv in &cfg.extra_params {
        if kv.enabled && !kv.key.trim().is_empty() {
            form.push((kv.key.clone(), kv.value.clone()));
        }
    }
}

pub(super) async fn fetch_via_http(cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    let form = build_grant_form(cfg);
    token_request(cfg, &form).await
}

pub(super) async fn refresh_via_http(
    cfg: &OAuth2Auth,
    refresh_token: &str,
) -> Result<OAuth2Token, AppError> {
    let form = build_refresh_form(cfg, refresh_token);
    token_request(cfg, &form).await
}

async fn token_request(
    cfg: &OAuth2Auth,
    form: &[(String, String)],
) -> Result<OAuth2Token, AppError> {
    let mut headers: Vec<(String, String)> = vec![
        (
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ),
        ("Accept".into(), "application/json".into()),
    ];
    // 走 Basic 时凭据不进 body。RFC 6749 §2.3.1 要求先对两个值做
    // form 编码再 base64 —— 直接 base64(`id:secret`) 会在 secret 含
    // `:` / `+` / 空格时被服务端解出另一个值。
    if cfg.client_auth == OAuth2ClientAuth::Basic
        && !cfg.client_id.trim().is_empty()
        && !cfg.client_secret.is_empty()
    {
        let raw = format!(
            "{}:{}",
            form_encode(cfg.client_id.trim()),
            form_encode(&cfg.client_secret)
        );
        headers.push((
            "Authorization".into(),
            format!("Basic {}", crate::base64::encode(raw.as_bytes())),
        ));
    }

    let input = HttpRequestInput {
        method: "POST".into(),
        url: cfg.token_url.trim().to_string(),
        headers,
        body: Some(encode_form(form)),
        timeout_ms: TOKEN_TIMEOUT_MS,
        // 这次请求带着 `client_secret` / 用户密码 / 授权码，跟随重定向等于把它们再寄给
        // 一个未校验的目标——`OutboundPolicy` 只挡私有网段，挡不住 302 到另一个公网主机。
        // token 端点本就不该重定向（RFC 6749 §3.2 的 back-channel POST），所以 3xx 由
        // `parse_token_response` 按非 2xx 报错，比悄悄换主机发出去好。
        follow_redirects: false,
        max_response_size_kb: TOKEN_MAX_RESPONSE_KB,
    };

    let result = http_executor::execute(&input).await;
    let resp = result.response.ok_or_else(|| {
        let msg = result
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| "token 请求失败".into());
        AppError::Http(msg)
    })?;
    parse_token_response(resp.status, &resp.body, &resp.content_type)
}
