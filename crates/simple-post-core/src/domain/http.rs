//! HTTP 方法：线格式上的大写动词 + RFC 9110 `token` 校验。

use serde::Serialize;

/// TS 侧 `HttpMethod` 的镜像。序列化为渲染层已在用的大写动词（`"GET"`、`"POST"`…）；
/// 未知动词落到 `Custom(String)`，使 UI 的前向兼容写入不会在 Rust 侧炸掉。
///
/// `Deserialize` 手写，好让本类型只有**一个**解析器：派生实现会把 `"get"` 收成
/// `Custom("get")` 并原样存库，于是 IPC 写入与 MCP 调用同一个动词会存出两种表示。
/// 一切都走 [`parse_http_method`]。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    /// 标准列表以外的动词（如 `PROPFIND`、`LOCK`）。序列化为裸字符串：枚举的 `UPPERCASE`
    /// 规则会重命名上面的单元变体，而不加 `untagged` 这个会变成 `{"CUSTOM":"PROPFIND"}`。
    /// 配套的**读**侧是下面的手写 `Deserialize`，它不需要这个属性。
    #[serde(untagged)]
    Custom(String),
}

impl<'de> serde::Deserialize<'de> for HttpMethod {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        parse_http_method(&raw).map_err(serde::de::Error::custom)
    }
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Custom(v) => v.as_str(),
        }
    }
}

/// RFC 9110 `token`：方法允许出现的全部字符。空格、中日韩字符、控制字节在此拒绝，
/// 否则会一路带到 `reqwest`，在发送那一刻才变成一个不透明的传输错误。
fn is_method_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// 每个 MCP 工具、请求执行路径**以及** `HttpMethod` 的 `Deserialize` 共用的解析器。
/// 已知动词大小写不敏感地映射；其余 RFC 9110 token（如 `PROPFIND`、`LOCK`）原样流成
/// `HttpMethod::Custom`，使将来/冷门的 HTTP 动词不丢数据。空串、含空白、非 token 的输入
/// 以 `AppError::Validation` 拒绝，好让调用方（通常是 LLM）拿到一条说明问题的消息。
pub fn parse_http_method(m: &str) -> Result<HttpMethod, crate::error::AppError> {
    use crate::error::AppError;
    let trimmed = m.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("method must not be empty".into()));
    }
    if !is_method_token(trimmed) {
        return Err(AppError::Validation(format!(
            "method {trimmed:?} is not a valid HTTP token"
        )));
    }
    Ok(match trimmed.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        other => HttpMethod::Custom(other.into()),
    })
}
