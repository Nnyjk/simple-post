//! 把 agent 写的输入强制转换成域类型。
//!
//! 这一层住在 core 而不是 MCP crate 里，因为两条写路径要遵守同一套规则：
//! `endpoint_create` 收一份扁平参数，`spec_upsert` 收一份嵌套规格，而它们对
//! 「`KeyValue` 的 `id` 缺失怎么办」「载荷长得像一份 Schema 怎么办」必须有同一个答案。
//! 规则写两遍就会漂移，而漂移的表现是「同一个字段在两条路上行为不同」——
//! 那是最难查的一类缺陷，因为两边都能自测通过。
//!
//! 三处放宽的理由各不相同：body 与 auth 用真 `Result` 解析（写错必须响亮报错），
//! 而 `KeyValue` 的 `id` / `enabled` 是 agent 没理由知道的簿记字段，给默认值即可。

use serde_json::Value;

use crate::domain::{AuthConfig, AuthType, BodyMode, KeyValue, RequestBody};
use crate::error::AppError;

/// 线上接受的 `body_mode` 取值，与 [`BodyMode`] 的变体一一对应；JSON Schema 与解析器
/// 共用这一份，改一处即可。
pub const BODY_MODE_VALUES: &[&str] = &["none", "json", "form", "multipart", "raw"];

/// 线上接受的 `auth_type` 取值，对应 [`AuthType`] 的小写变体名。
pub const AUTH_TYPE_VALUES: &[&str] = &["none", "bearer", "basic", "apikey", "oauth2"];

/// 解析 `body_mode`。未知取值报错并列出全部合法取值。
pub fn parse_body_mode(s: &str) -> Result<BodyMode, AppError> {
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

/// 按 `auth_type` 选方案，按 `auth_config` 取凭据，合成一份 [`AuthConfig`]。
///
/// `auth_config` 用真 `Result` 反序列化，绝不 `unwrap_or_default()`：配置写错时
/// 静默退化成全空 `AuthConfig`，agent 会拿到一个 SUCCESS 响应，里面描述的却是
/// 根本没有 token 的 bearer。
///
/// `kind` 经 [`AuthConfigWire`] 而不是直接进 [`AuthConfig`]：域类型在 `kind` 上没有
/// serde 默认值，于是 `{"bearer":"tok"}` —— JSON Schema 文档化的、agent 实际会
/// 写的那种形状 —— 会因缺字段而失败。方案真正是在 `auth_type` 里选的，下面会
/// 写回结果，所以 config 里的 `kind` 是冗余而非必需。
pub fn build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<AuthConfig, AppError> {
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
    kind: Option<AuthType>,
    #[serde(default)]
    bearer: Option<String>,
    #[serde(default)]
    basic: Option<crate::domain::BasicAuth>,
    #[serde(default)]
    apikey: Option<crate::domain::ApiKeyAuth>,
    #[serde(default)]
    oauth2: Option<crate::domain::OAuth2Auth>,
}

impl AuthConfigWire {
    fn into_auth_config(self) -> AuthConfig {
        AuthConfig {
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
/// [`KeyValue`] 的 `id` 与 `enabled` 没有 serde 默认值，于是最自然的写法
/// `{"key":"page","value":"1"}` 反序列化失败。两处放宽的理由不同：
///
/// * `id` 可选，缺失时另铸。纯簿记字段 —— UI 拿它在 React 里做行 key，请求组装不读。
///   客户端真传了就保留，于是把上一行原样回传的编辑是就地更新，不会把它变成孤儿。
/// * `enabled` 可选，且**默认 `true`**，不是 `KeyValue` 自己的 `Default`（`false`）。
///   存成禁用的请求头就是一个永远发不出去的请求头；agent 点名一个请求头就是要发它。
///   显式 `false` 仍然照办，那对应「留着但先关掉」。
///
/// `value` 用同样方式放宽，因为「故意发一个空请求头值」是真事，不是笔误。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueInput {
    #[serde(default)]
    pub id: Option<String>,
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
}

impl KeyValueInput {
    /// 转成域类型：`id` 缺失时另铸，`enabled` 缺省为 `true`。
    pub fn into_key_value(self) -> KeyValue {
        let id = match self.id {
            Some(id) if !id.trim().is_empty() => id,
            _ => crate::domain::uid(),
        };
        KeyValue {
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
pub fn coerce_key_values(rows: Vec<KeyValueInput>) -> Vec<KeyValue> {
    rows.into_iter()
        .map(KeyValueInput::into_key_value)
        .collect()
}

/// 载荷与定义写反了就地拒绝。
///
/// 请求体有两个字段：`content` 是这一次发出的载荷，`schema` 是文档里那份参数定义。
/// 两者都是 JSON 对象，形状接近，于是 agent 写接口定义时容易把 Schema 塞进
/// `content` —— 每次调试都把一份 Schema 当请求体发出去，而接口的文档里空空如也。
///
/// 判据是启发式的（见 [`RequestBody::content_looks_like_schema`]），
/// 故只在「这份 body 看起来是 Schema、且还没填 `schema`」时才拒绝：确实要把一份
/// Schema 当载荷发的接口，可以把同样的内容也写进 `schema`，把这份意图写明。
pub fn reject_schema_shaped_payload(body: &RequestBody) -> Result<(), AppError> {
    if !body.payload_may_be_a_schema() {
        return Ok(());
    }
    Err(AppError::Validation(
        "body_content looks like a JSON Schema (a top-level \"type\" together with \
         \"properties\"), not a request payload. body_content is the body that is \
         actually sent; the documented parameter definition belongs in body_schema. \
         If this was the definition, pass it as body_schema and leave body_content \
         empty. If the payload really is a schema document, set body_schema as well \
         so the intent is explicit."
            .into(),
    ))
}

/// 检查一份 [`AuthConfig`] 的凭据是否与它声明的方案对得上。
///
/// 方案说 bearer 而没有 token，这份配置发出去就是一个空的 `Authorization` 头，
/// 而服务端只会回一个 401 —— 与其让 agent 跑到那一步才发现，不如在落库前就说清。
/// 凭据写成 `{{变量}}` 是允许的：那时值在发送时从环境里取。
pub fn check_auth_payload(auth: &AuthConfig) -> Result<(), AppError> {
    let missing = match auth.kind {
        AuthType::None | AuthType::Oauth2 => None,
        AuthType::Bearer => match auth.bearer.as_deref() {
            Some(t) if !t.trim().is_empty() => None,
            _ => Some("a bearer token"),
        },
        AuthType::Basic => match auth.basic.as_ref() {
            Some(b) if !b.username.trim().is_empty() => None,
            _ => Some("a username"),
        },
        AuthType::Apikey => match auth.apikey.as_ref() {
            Some(k) if !k.key.trim().is_empty() => None,
            _ => Some("an api key name"),
        },
    };
    match missing {
        None => Ok(()),
        Some(what) => Err(AppError::Validation(format!(
            "auth.kind is '{}' but the config has no {what}; \
             fill the credential, or leave `auth` out entirely when the endpoint \
             needs no credentials",
            auth.kind.as_str()
        ))),
    }
}
