//! 与 token 端点的那一次往返：拼请求、发请求、解包应答。
//!
//! 协议机制——`grant_type`、client 凭据放 Basic 头还是请求体、PKCE 的
//! `code_verifier`、`extra_params`、form 编码——全部由 `oauth2` crate
//! （[ramosbugs/oauth2-rs](https://github.com/ramosbugs/oauth2-rs)）组装。本文件只留
//! 三件 crate 不管的事：
//!
//! 1. 把 [`OAuth2Auth`] 翻成 crate 的请求类型（配置里是空格分隔的 scope 整串，
//!    crate 收的是逐个 `Scope`）；
//! 2. 出站交给 [`http_client::ExecutorClient`]，让 token 端点与普通请求共用同一道
//!    [`crate::http_executor`]——SSRF 策略、超时、禁止重定向、体积上限都在那里；
//! 3. 把 crate 的应答映射回 [`OAuth2Token`]，保住本项目的中文错误文案。

use oauth2::basic::{
    BasicErrorResponseType, BasicTokenIntrospectionResponse, BasicTokenResponse, BasicTokenType,
};
use oauth2::{
    AuthType, AuthorizationCode, Client, ClientId, ClientSecret, EndpointNotSet, EndpointSet,
    PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, ResourceOwnerPassword,
    ResourceOwnerUsername, Scope, StandardErrorResponse, StandardRevocableToken, TokenResponse,
    TokenUrl,
};

use crate::domain::{OAuth2Auth, OAuth2ClientAuth, OAuth2GrantType};
use crate::error::AppError;

use super::http_client::ExecutorClient;
use super::parse::extract_code;
use super::store::unix_secs;
use super::OAuth2Token;

/// crate 的错误应答类型。
type ErrorResp = StandardErrorResponse<BasicErrorResponseType>;

/// crate 的 `Client` 有 9 个必填泛型参数，别在签名里逐个写出来。
///
/// 第 10 位是 token 端点的状态：`set_token_uri` 之后就是 [`EndpointSet`]，故四个
/// `exchange_*` 是不带 `Result` 的那组。中间四位是 auth / device / introspection /
/// revocation 四个端点的状态，本项目一个都不用，全留 [`EndpointNotSet`]。
type Oauth2Client = Client<
    ErrorResp,
    BasicTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    ErrorResp,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

/// 组一个已配好 token 端点与 client 凭据的 client。
///
/// `AuthType` 直接对应配置里的 `client_auth`：RFC 6749 §2.3.1 允许凭据放 Basic 头
/// 或请求体，crate 按 `AuthType` 决定放哪儿。Basic 分支**先对 id 与 secret 各做
/// 一次 form 编码再 base64**（§2.3.1 的原文要求，不是 RFC 7617 Basic 的通常做法），
/// 所以含 `:` / `+` / 空格的 secret 也不会被服务端解出另一组凭据。
pub(super) fn client_for(cfg: &OAuth2Auth) -> Result<Oauth2Client, AppError> {
    let mut client = Client::new(ClientId::new(cfg.client_id.trim().to_string()))
        .set_token_uri(TokenUrl::new(cfg.token_url.trim().to_string()).map_err(config_error)?)
        .set_auth_type(match cfg.client_auth {
            OAuth2ClientAuth::Basic => AuthType::BasicAuth,
            OAuth2ClientAuth::Body => AuthType::RequestBody,
        });
    if !cfg.client_secret.is_empty() {
        client = client.set_client_secret(ClientSecret::new(cfg.client_secret.clone()));
    }
    Ok(client)
}

/// `TokenUrl` / `RedirectUrl` 解析失败只可能是「不是合法绝对 URL」，而
/// [`super::validate`] 已经先报过一句中文的「需要 Token URL」，故这里给短文案。
fn config_error(e: oauth2::url::ParseError) -> AppError {
    AppError::Validation(format!("token 端点或 redirect_uri 不是合法 URL: {e}"))
}

/// 停用或空 key 的 `extra_params` 不该发出——停用是用户在界面上显式关掉的。
pub(crate) fn extra_params(cfg: &OAuth2Auth) -> Vec<(String, String)> {
    cfg.extra_params
        .iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (kv.key.clone(), kv.value.clone()))
        .collect()
}

/// 配置里的 scope 是空格分隔的一整串（`openid profile email`），crate 收的是逐个
/// `Scope`，故拆开逐个挂。
pub(crate) fn scopes_of(cfg: &OAuth2Auth) -> Vec<Scope> {
    cfg.scope
        .split_whitespace()
        .map(|s| Scope::new(s.to_string()))
        .collect()
}

/// 给请求挂 scope 与 `extra_params`。
///
/// 四个 crate 请求类型没有共同 trait，而这些方法四边同名，故宏写一次。
/// `scope` 分支走 `add_scope`（crate 原生字段）；`no_scope` 用于
/// `CodeTokenRequest`——它没有 `add_scope`，scope 按普通表单字段挂（RFC 6749
/// §4.1.3 允许 token 请求带 scope）。
///
/// 路径一律写全 `$crate::`：宏在调用处展开，裸名字会按**调用方**的作用域解析，
/// 而调用方（本文件的单测）没有 `use` 过这两个函数。
macro_rules! decorate {
    ($r:ident, $cfg:expr, scope) => {{
        for s in $crate::oauth2::token_endpoint::scopes_of($cfg) {
            $r = $r.add_scope(s);
        }
        for (k, v) in $crate::oauth2::token_endpoint::extra_params($cfg) {
            $r = $r.add_extra_param(k, v);
        }
    }};
    ($r:ident, $cfg:expr, no_scope) => {{
        for (k, v) in $crate::oauth2::token_endpoint::extra_params($cfg) {
            $r = $r.add_extra_param(k, v);
        }
    }};
}

/// 单测要断言 crate 真正发出的 wire 形状，故把这个宏交给测试。线上各处都写在本
/// 文件内，不需要这条 re-export。
#[cfg(test)]
pub(crate) use decorate;

/// 一次换 token（三种有 token 端点的流程）。
pub(super) async fn fetch_via_http(cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    let client = client_for(cfg)?;

    let resp = match cfg.grant_type {
        OAuth2GrantType::ClientCredentials => {
            let mut r = client.exchange_client_credentials();
            decorate!(r, cfg, scope);
            r.request_async(&ExecutorClient)
                .await
                .map_err(map_request_error)?
        }
        OAuth2GrantType::AuthorizationCode => {
            let mut r = client.exchange_code(AuthorizationCode::new(extract_code(&cfg.auth_code)));
            // 留空就不挂：PKCE 的 verifier 由用户在 IdP 侧登记对应的 challenge，
            // 无配置时发一个空的 `code_verifier=` 会被 IdP 当成配错了。
            if !cfg.code_verifier.trim().is_empty() {
                r = r
                    .set_pkce_verifier(PkceCodeVerifier::new(cfg.code_verifier.trim().to_string()));
            }
            if !cfg.redirect_uri.trim().is_empty() {
                let uri =
                    RedirectUrl::new(cfg.redirect_uri.trim().to_string()).map_err(config_error)?;
                r = r.set_redirect_uri(std::borrow::Cow::Owned(uri));
            }
            if !cfg.scope.trim().is_empty() {
                r = r.add_extra_param("scope", cfg.scope.trim().to_string());
            }
            decorate!(r, cfg, no_scope);
            r.request_async(&ExecutorClient)
                .await
                .map_err(map_request_error)?
        }
        OAuth2GrantType::Password => {
            let username = ResourceOwnerUsername::new(cfg.username.trim().to_string());
            // 密码按原样发，不 trim：是否接受带空格的密码是 IdP 的决定。
            let password = ResourceOwnerPassword::new(cfg.password.clone());
            let mut r = client.exchange_password(&username, &password);
            decorate!(r, cfg, scope);
            r.request_async(&ExecutorClient)
                .await
                .map_err(map_request_error)?
        }
        // 到不了这里：`implicit` 没有 token 端点，`validate` 先拦下。
        OAuth2GrantType::Implicit => return Err(implicit_has_no_token_endpoint()),
    };

    Ok(to_token(resp))
}

/// 用 `refresh_token` 换新的 access token（RFC 6749 §6）。
pub(super) async fn refresh_via_http(
    cfg: &OAuth2Auth,
    refresh_token: &str,
) -> Result<OAuth2Token, AppError> {
    let client = client_for(cfg)?;
    let token = RefreshToken::new(refresh_token.to_string());
    let mut r = client.exchange_refresh_token(&token);
    decorate!(r, cfg, scope);
    let resp = r
        .request_async(&ExecutorClient)
        .await
        .map_err(map_request_error)?;
    Ok(to_token(resp))
}

/// crate 的英文错误 → 本项目的中文错误。
///
/// 传输级失败与**非 2xx** 已在 [`http_client`] 里就地变成 `AppError`，crate 根本没
/// 见到非 2xx 应答，所以走不到 `ServerResponse`。剩下的 `Parse` 就是「2xx 但
/// body 里没有可用的 access_token」。
fn map_request_error(e: RequestTokenError<AppError, ErrorResp>) -> AppError {
    match e {
        RequestTokenError::Request(inner) => inner,
        RequestTokenError::Parse(_, _) => {
            AppError::Validation("token 响应里没有 access_token".into())
        }
        // 非 2xx 已在 bridge 里变成 `AppError` 了，走不到这里；留着是因为
        // crate 的类型签名要求处理这一支。
        RequestTokenError::ServerResponse(resp) => {
            AppError::Validation(format!("token 端点返回 {:?}", resp.error()))
        }
        RequestTokenError::Other(msg) => AppError::Validation(format!("token 请求失败: {msg}")),
    }
}

fn implicit_has_no_token_endpoint() -> AppError {
    AppError::Validation("implicit 流程没有 token 端点".into())
}

/// crate 的 `BasicTokenResponse` → 本项目的 [`OAuth2Token`]。
pub(super) fn to_token(resp: BasicTokenResponse) -> OAuth2Token {
    let now = unix_secs();
    OAuth2Token {
        access_token: resp.access_token().secret().clone(),
        // 刷新响应按 §6 可能不重复 refresh_token，调用方负责沿用旧值。
        refresh_token: resp.refresh_token().map(|t| t.secret().clone()),
        // RFC 6749 §5.1：IdP 不给 `token_type` 时按 `Bearer` 处理。
        token_type: match resp.token_type() {
            BasicTokenType::Bearer => "Bearer".to_string(),
            BasicTokenType::Mac => "MAC".to_string(),
            BasicTokenType::Extension(s) => s.clone(),
        },
        scope: resp
            .scopes()
            .map(|s| s.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty()),
        // `expires_in` 缺失 = 不知道何时过期。别拿 0 当「已过期」。
        expires_at: resp
            .expires_in()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .filter(|s| *s > 0)
            .map(|s| now + s),
        obtained_at: now,
    }
}
