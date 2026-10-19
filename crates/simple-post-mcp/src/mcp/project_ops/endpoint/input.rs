//! 把 agent 的输入强制转换成域类型：`body_mode`、`auth_config`、`KeyValue` 行。
//!
//! 三处放宽的理由各不相同：body 与 auth 用真 `Result` 解析（写错必须响亮报错），
//! 而 `KeyValue` 的 `id` / `enabled` 是 agent 没理由知道的簿记字段，给默认值即可。

use serde_json::{json, Value};

use simple_post_core::error::AppError;
// =========================================================================
// 私有辅助函数（只被上面的 endpoint_* 工具使用）
// =========================================================================

pub(super) fn parse_body_mode(s: &str) -> Result<simple_post_core::domain::BodyMode, AppError> {
    use simple_post_core::domain::BodyMode;
    Ok(match s.to_ascii_lowercase().as_str() {
        "none" => BodyMode::None,
        "json" => BodyMode::Json,
        "form" => BodyMode::Form,
        "multipart" => BodyMode::Multipart,
        "raw" => BodyMode::Raw,
        other => {
            return Err(AppError::Validation(format!(
                "unknown body_mode {other:?}; expected one of: {}",
                BODY_MODE_VALUES.join(", ")
            )))
        }
    })
}

/// 线上接受的 `body_mode` 取值，与 [`simple_post_core::domain::BodyMode`] 的变体
/// 一一对应；JSON Schema 与解析器共用这一份，改一处即可。
pub(super) const BODY_MODE_VALUES: &[&str] = &["none", "json", "form", "multipart", "raw"];

/// 线上接受的 `auth_type` 取值，对应 [`simple_post_core::domain::AuthType`] 的小写
/// 变体名。
pub(super) const AUTH_TYPE_VALUES: &[&str] = &["none", "bearer", "basic", "apikey", "oauth2"];

/// OAuth2 载荷的 JSON Schema。两处用它（`endpoint_update` 的 `auth.oauth2` 与
/// `endpoint_create` 的 `auth_config` 说明），故 AI 客户端在两条路上看到同一份形状。
///
/// `additionalProperties: false` 是承重的：`endpoint_update` 的 `auth` 上有它，
/// 而 agent 是这里最主要的外部写路径。漏登记一个字段不是「宽松一点」，而是那个字段
/// **根本传不进来**——一个仍然看着正确注册的工具却永远收不到配置。
pub(super) fn oauth2_config_schema() -> Value {
    json!({
        "type": "object",
        "description": "OAuth2 settings. The token is fetched on demand and \
                        refreshed when it expires; authorization_code needs \
                        authCode pasted after the user authorises in a browser, \
                        and implicit has no token endpoint at all (the token \
                        comes back in the redirect fragment).",
        "properties": {
            "grantType": {
                "type": "string",
                "enum": ["authorization_code", "client_credentials", "password", "implicit"]
            },
            "tokenUrl": { "type": "string" },
            "clientId": { "type": "string" },
            "clientSecret": { "type": "string" },
            "clientAuth": {
                "type": "string",
                "enum": ["basic", "body"],
                "description": "Where the client credentials go: an HTTP Basic header (default) or the form body."
            },
            "scope": { "type": "string", "description": "Space separated." },
            "authorizationUrl": { "type": "string" },
            "redirectUri": { "type": "string" },
            "codeVerifier": { "type": "string", "description": "PKCE verifier. Must match the challenge registered with the IdP; this app never generates one." },
            "authCode": { "type": "string", "description": "One-shot authorization code, or the whole redirect URL carrying ?code=." },
            "username": { "type": "string" },
            "password": { "type": "string" },
            "extraParams": {
                "type": "array",
                "description": "Extra form fields appended to the token request (e.g. audience).",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "key": { "type": "string" },
                        "value": { "type": "string" },
                        "enabled": { "type": "boolean" }
                    },
                    "required": ["key", "value"],
                    "additionalProperties": false
                }
            },
            "tokenLocation": { "type": "string", "enum": ["header", "query"] },
            "tokenParam": { "type": "string", "description": "Query parameter name for tokenLocation=query. Defaults to access_token." }
        },
        "required": ["grantType"],
        "additionalProperties": false
    })
}

pub(super) fn build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<simple_post_core::domain::AuthConfig, AppError> {
    use simple_post_core::domain::{AuthConfig, AuthType};
    let kind = match auth_type.unwrap_or("none").to_ascii_lowercase().as_str() {
        "none" => AuthType::None,
        "bearer" => AuthType::Bearer,
        "basic" => AuthType::Basic,
        "apikey" => AuthType::Apikey,
        "oauth2" => AuthType::Oauth2,
        other => {
            return Err(AppError::Validation(format!(
                "unknown auth_type {other:?}; expected one of: {}",
                AUTH_TYPE_VALUES.join(", ")
            )))
        }
    };
    let cfg = auth_config.unwrap_or(Value::Null);
    // `auth_config` 用真 `Result` 反序列化，绝不 `unwrap_or_default()`：配置写错时
    // 静默退化成全空 `AuthConfig`，agent 会拿到一个 SUCCESS 响应，里面描述的却是
    // 根本没有 token 的 bearer。
    //
    // `kind` 经 `AuthConfigWire` 而不是直接进 `AuthConfig`：域类型在 `kind` 上没有
    // serde 默认值，于是 `{"bearer":"tok"}` —— JSON Schema 文档化的、agent 实际会
    // 写的那种形状 —— 会因缺字段而失败。方案真正是在 `auth_type` 里选的，下面会
    // 写回结果，所以 config 里的 `kind` 是冗余而非必需。
    let mut out: AuthConfig = match cfg {
        Value::Null => AuthConfig::default(),
        other => {
            let wire: AuthConfigWire = serde_json::from_value(other).map_err(|e| {
                AppError::Validation(format!(
                    "auth_config is malformed ({e}); expected an object like \
                     {{\"bearer\":\"<token>\"}} for bearer, \
                     {{\"basic\":{{\"username\":\"u\",\"password\":\"p\"}}}} for basic, \
                     {{\"apikey\":{{\"key\":\"X-Api-Key\",\"value\":\"<key>\",\
                     \"inLocation\":\"header\"}}}} for apikey, \
                     {{\"oauth2\":{{\"grantType\":\"client_credentials\",\
                     \"tokenUrl\":\"https://idp.example.com/oauth/token\",\
                     \"clientId\":\"<id>\"}}}} for oauth2"
                ))
            })?;
            wire.into_auth_config()
        }
    };
    out.kind = kind;
    Ok(out)
}

/// `kind` 可选的 `AuthConfig` —— 理由见 [`build_auth`]。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthConfigWire {
    #[serde(default)]
    kind: Option<simple_post_core::domain::AuthType>,
    #[serde(default)]
    bearer: Option<String>,
    #[serde(default)]
    basic: Option<simple_post_core::domain::BasicAuth>,
    #[serde(default)]
    apikey: Option<simple_post_core::domain::ApiKeyAuth>,
    #[serde(default)]
    oauth2: Option<simple_post_core::domain::OAuth2Auth>,
}

impl AuthConfigWire {
    fn into_auth_config(self) -> simple_post_core::domain::AuthConfig {
        simple_post_core::domain::AuthConfig {
            // 由调用方按 `auth_type` 覆写；保留它只为让「完全不带 `auth_type`、
            // 但 config 自带 `kind`」的形状能原样往返。
            kind: self.kind.unwrap_or_default(),
            bearer: self.bearer,
            basic: self.basic,
            apikey: self.apikey,
            oauth2: self.oauth2,
        }
    }
}

/// agent 手写的一行 `KeyValue` 的线上形状。
///
/// [`simple_post_core::domain::KeyValue`] 的 `id` 与 `enabled` 没有 serde 默认值，
/// 于是最自然的写法 `{"key":"page","value":"1"}` 反序列化失败。两处放宽的理由不同：
///
/// * `id` 可选，缺失时另铸。纯簿记字段 —— UI 拿它在 React 里做行 key，请求组装不读。
///   客户端真传了就保留，于是把上一行原样回传的编辑是就地更新，不会把它变成孤儿。
/// * `enabled` 可选，且**默认 `true`**，不是 `KeyValue` 自己的 `Default`（`false`）。
///   存成禁用的请求头就是一个永远发不出去的请求头；agent 点名一个请求头就是要发它。
///   显式 `false` 仍然照办，那对应「留着但先关掉」。
///
/// `value` 用同样方式放宽，因为「故意发一个空请求头值」是真事，不是笔误。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KeyValueInput {
    #[serde(default)]
    id: Option<String>,
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    description: Option<String>,
}

impl KeyValueInput {
    fn into_key_value(self) -> simple_post_core::domain::KeyValue {
        let id = match self.id {
            Some(id) if !id.trim().is_empty() => id,
            _ => simple_post_core::domain::uid(),
        };
        simple_post_core::domain::KeyValue {
            id,
            key: self.key,
            value: self.value,
            enabled: self.enabled.unwrap_or(true),
            description: self.description,
        }
    }
}

/// 反序列化 `params` / `headers` 数组，并补齐 `KeyValue` 要求、而 agent 没有理由
/// 知道的那些簿记字段。
pub(super) fn coerce_key_values(
    rows: Vec<KeyValueInput>,
) -> Vec<simple_post_core::domain::KeyValue> {
    rows.into_iter()
        .map(KeyValueInput::into_key_value)
        .collect()
}
