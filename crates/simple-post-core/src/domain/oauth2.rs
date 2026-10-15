//! OAuth2 配置载荷（存进 `auth_json` 的那一半）。

use serde::{Deserialize, Serialize};

use super::auth::KeyValue;

/// OAuth2 授权流程（grant type）。四种的取 token 方式不同，其余字段共用。
///
/// 线上是 snake_case（`authorization_code` / `client_credentials` /
/// `password` / `implicit`）—— 与 RFC 6749 的 `grant_type` 取值同名，AI 客户端
/// 与用户都不必再做一次心算映射。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2GrantType {
    /// 授权码。用户自己在浏览器里换到 `auth_code`，本应用**不**开回环端口。
    #[default]
    AuthorizationCode,
    /// 客户端凭证。机器对机器，无用户参与——唯一全程无需人工的一档。
    ClientCredentials,
    /// 资源所有者密码（ROPC）。RFC 9700 已不推荐新接入，保留是因为大量老 IdP
    /// 只有这一档可用。
    Password,
    /// 隐式。token 由浏览器 fragment 直接带回，无 token 端点、**永远无法刷新**。
    Implicit,
}

/// 客户端凭据怎么送到 token 端点。线上 snake_case。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2ClientAuth {
    /// `Authorization: Basic base64(urlencode(id):urlencode(secret))`，RFC 6749 §2.3.1
    /// 的默认。多数 IdP 只认这一档。
    #[default]
    Basic,
    /// `client_id` / `client_secret` 进 form body。public client（无 secret）
    /// 与少数只读 body 的 IdP 需要它。
    Body,
}

/// access token 挂哪。与 `ApiKeyLocation` 同构，线上 snake_case。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2TokenLocation {
    #[default]
    Header,
    Query,
}

/// OAuth2 配置载荷。
///
/// **每个字段都 `#[serde(default)]`，且刻意不做必填校验的反序列化期拒绝。**
/// 这一行住在 `endpoint_details.auth_json` 里，库里现存的 `'{}'` 默认行与
/// `{kind: bearer, bearer: "…"}` 都必须继续读得出来——把新变体的字段设成必填，
/// 就是 AGENTS.md 记的「`type` → `kind` 改名后旧行读不出来」的同一类事故，
/// 而那次是靠 bump 版本删库才摆脱的。缺字段推迟到真正发请求时按
/// [`crate::oauth2::validate`] 报错，那时报错信息比 serde 的字段列表有用。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2Auth {
    #[serde(default)]
    pub grant_type: OAuth2GrantType,
    /// token 端点。`implicit` 不用，置空即可。
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub client_auth: OAuth2ClientAuth,
    /// 空格分隔。同名 scope 才覆盖服务端默认值，故默认空串而非 null。
    #[serde(default)]
    pub scope: String,
    /// `authorization_code` / `implicit` 用：给用户点开的登录地址。只存着，
    /// 本应用不主动打开浏览器（不开回环端口，见 issue #2 范围外）。
    #[serde(default)]
    pub authorization_url: String,
    /// `authorization_code` 换 token 时按 RFC 要求回传，故可留空（IdP 多接受）。
    #[serde(default)]
    pub redirect_uri: String,
    /// PKCE 的 verifier。**不做自动生成与浏览器回环**：用户在 IdP 侧登记的
    /// challenge 必须与这里一致，留给懂行的人填。
    #[serde(default)]
    pub code_verifier: String,
    /// `authorization_code` 的一次性输入。用户在浏览器登录后拿到 code（或整条
    /// 带 `?code=` 的回调 URL），贴进来由后端换 token。刻意持久化：MCP 侧
    /// `endpoint_create` 要能写出可用的端点，而它没有浏览器的临时态可依附。
    #[serde(default)]
    pub auth_code: String,
    /// `password` 流程的用户名 / 密码。
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// 追加进 token 请求的额外表单字段。Azure 要 `audience`、Okta 要
    /// `audience` / `organization`，没有这一层就得改代码。
    #[serde(default)]
    pub extra_params: Vec<KeyValue>,
    #[serde(default)]
    pub token_location: OAuth2TokenLocation,
    /// query 档的参数名。默认 `access_token`。
    #[serde(default)]
    pub token_param: String,
}

impl OAuth2Auth {
    /// `token_param` 为空时的兜底名。
    pub const DEFAULT_TOKEN_PARAM: &'static str = "access_token";

    /// 实际使用的 query 参数名。
    pub fn token_param_or_default(&self) -> &str {
        if self.token_param.trim().is_empty() {
            Self::DEFAULT_TOKEN_PARAM
        } else {
            self.token_param.as_str()
        }
    }

    /// `implicit` 能否刷新。不能——RFC 6749 §4.2.2 明说隐式流不签发 refresh token。
    pub fn can_refresh(&self) -> bool {
        self.grant_type != OAuth2GrantType::Implicit
    }
}
