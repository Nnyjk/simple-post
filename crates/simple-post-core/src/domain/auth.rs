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
    ///
    /// 内容是**结构化的**：[`RequestBody::content`] 装的是一行一行的键值对（JSON 数组
    /// 文本），不是一个拼好的 urlencoded 串。行里带着说明，界面上的表格才有地方放。
    Form,
    /// `multipart/form-data`。线上是 `multipart`。
    ///
    /// 与 [`BodyMode::Form`] 是两种东西：multipart 的 Content-Type 必须带 boundary，
    /// 而 boundary 每次请求现生成，所以 Content-Type 不可能是静态返回值。分片结构存
    /// 在 [`RequestBody::content`] 里，是一个 JSON 数组；装配在
    /// `src/lib/multipart.ts` 与 `mcp/http_ops/multipart.rs`。
    Multipart,
    /// `application/xml`。内容是 XML 原文。
    Xml,
    /// `text/plain`。内容是纯文本原文。
    Raw,
    /// `application/octet-stream`。整个请求体是一个文件。
    ///
    /// 内容是 [`RequestBody::content`] 里那份文件描述（JSON 对象文本），描述里带着文件名、
    /// 字节数与 base64 的字节。文件本身不在库里存路径——路径由**运行请求的那台机器**
    /// 解析，桌面端与服务端容器不是一个文件系统。
    Binary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    pub mode: BodyMode,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

/// `form` 模式里的一行。
///
/// 键、值、是否参与发送、说明。`enabled` 缺省为真：一份没写 `enabled` 的行就是一行要
/// 发的表单字段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 缺省为真的布尔值。
///
/// serde 的 `default` 要一个函数，故单独给它一个名字：`FormField::enabled` 的缺省值是
/// 「参与发送」，不是「不参与」。
fn enabled_by_default() -> bool {
    true
}

/// `multipart` 模式里的一个分片。
///
/// 与 [`crate::domain::RequestBody::content`] 里那份 JSON 数组一一对应。文件分片的内容
/// 放在 [`MultipartPart::base64`] 里：文件是字节，而分片数组存在 JSON 文本里，故字节
/// 只能以 base64 的形态进去。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MultipartPart {
    /// 分片名（`Content-Disposition` 里的 `name`）。
    #[serde(default)]
    pub name: String,
    /// 文本分片的内容。
    #[serde(default)]
    pub value: String,
    /// 这一分片是否参与发送。缺省为真。
    ///
    /// 与 `FormField::enabled` 同一条规则：停用的分片不进 body，但还留在表格里，
    /// 用户改主意时勾回来即可，不必重新选文件。
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// 文件分片的文件名。有它即文件分片。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// 分片自己的 Content-Type。文件分片缺省为 `application/octet-stream`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// 文件分片的字节（base64）。有它时 [`MultipartPart::value`] 不参与发包。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    /// 说明。界面上那一列写着什么，不上线。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `binary` 模式选中的那个文件。
///
/// 存在 [`RequestBody::content`] 里，是一个 JSON 对象文本。`base64` 是文件的字节。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BodyFile {
    /// 文件名。界面上显示它；它不上线（请求体只有字节与 Content-Type）。
    #[serde(default)]
    pub name: String,
    /// 文件有多少字节。
    #[serde(default)]
    pub size: u64,
    /// 文件自己的 MIME 类型。缺省 `application/octet-stream`。
    #[serde(default)]
    pub content_type: String,
    /// 文件的字节（base64）。
    #[serde(default)]
    pub base64: String,
}

impl RequestBody {
    /// 这份 body 的内容看起来是一份 JSON Schema，而不是一份请求实例。
    ///
    /// 两者都是 JSON 对象，而 JSON Schema 惯用的顶层键（`type` / `properties` /
    /// `$schema` / `items` / `allOf` …）恰好也是业务载荷可能用的键，故这是启发式
    /// 判定，不是类型判定。它要挡的是一次具体的错位：把「这个接口接受什么参数」
    /// 写进了「这一次发什么」。
    ///
    /// 判据刻意收窄：只有当顶层同时出现 `type` 与 `properties`（或出现 `$schema`）
    /// 才算 schema。只出现 `type` 的载荷（`{"type":"order"}`）太常见，不能算。
    pub fn content_looks_like_schema(&self) -> bool {
        if self.mode != BodyMode::Json {
            return false;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.content) else {
            return false;
        };
        let Some(object) = value.as_object() else {
            return false;
        };
        if object.contains_key("$schema") {
            return true;
        }
        object.contains_key("type") && object.contains_key("properties")
    }

    /// 载荷与定义写反了：这份 body 的 `content` 看起来是一份 Schema，而 `schema` 还是空的。
    ///
    /// 填了 `schema` 就不算写反 —— 那时这份内容是有意选定的（真要把一份 Schema 当
    /// 载荷发的接口就是这种情况）。
    pub fn payload_may_be_a_schema(&self) -> bool {
        self.schema.as_deref().unwrap_or("").trim().is_empty() && self.content_looks_like_schema()
    }
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
    /// 关键字，用 serde `rename` 遮掩会让 Tauri command 参数与 REST body 各用一套名字。
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
