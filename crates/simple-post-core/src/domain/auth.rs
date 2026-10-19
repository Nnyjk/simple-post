//! 请求载荷共用的小类型：键值对、请求体与 auth 方案。

use serde::{Deserialize, Serialize};

use super::OAuth2Auth;

// 键值对

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub id: String,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// body

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BodyMode {
    #[default]
    None,
    Json,
    /// `application/x-www-form-urlencoded`。不是 multipart。
    Form,
    /// `multipart/form-data`。线上是 `multipart`。
    ///
    /// 与 [`BodyMode::Form`] 是两种东西：multipart 的 Content-Type 必须带 boundary，
    /// 而 boundary 每次请求现生成，所以 Content-Type 不可能是静态返回值。分片结构存
    /// 在 [`RequestBody::content`] 里，是一个 JSON 数组；装配在
    /// `src/lib/multipart.ts` 与 `mcp/http_ops/multipart.rs`。
    Multipart,
    Raw,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    pub mode: BodyMode,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

// auth

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    #[default]
    None,
    Bearer,
    Basic,
    Apikey,
    /// 线上是 `oauth2`（`rename_all = "lowercase"` 把 `Oauth2` 压成全小写，
    /// 不是 RFC 里的 `OAuth2`）。配置载荷在 [`AuthConfig::oauth2`]。
    Oauth2,
}

impl AuthType {
    /// 线上取值。与 `rename_all = "lowercase"` 一一对应，错误信息里要显示的是
    /// 用户在 JSON 里写的那个词，不是 Rust 的变体名。
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::None => "none",
            AuthType::Bearer => "bearer",
            AuthType::Basic => "basic",
            AuthType::Apikey => "apikey",
            AuthType::Oauth2 => "oauth2",
        }
    }
}

/// apikey 挂在哪。线上为 `inLocation: "header" | "query"`；
/// 为何不能用显然的 `in` 见模块文档。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
    /// 序列化为 `inLocation`。`in` 在 TypeScript 是保留字；`in_` 也不行——
    /// `rename_all = "camelCase"` 会把 `in_` 变回线上的 `in`，两种语言照样对不上。见模块文档。
    pub in_location: ApiKeyLocation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    /// 用的是哪种方案。线上叫 `kind`，因为这里字段就叫 `kind`：`type` 在 Rust 里同样是
    /// 关键字，用 serde `rename` 遮掩只会在 Tauri command 参数与 REST body 之间重新撕开一道缝。
    pub kind: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apikey: Option<ApiKeyAuth>,
    /// `kind == OAuth2` 时才读。刻意留成 `Option` 而不是让 `AuthConfig` 变成
    /// tagged union：`auth_json` 里存着四年来的 `'{}'` 默认行与旧的
    /// `{kind: bearer, bearer: …}`，非 tag 化的扁平布局让它们继续反序列化成功
    /// （见 [`OAuth2Auth`] 字段上的 `#[serde(default)]` 理由）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth2: Option<OAuth2Auth>,
}
