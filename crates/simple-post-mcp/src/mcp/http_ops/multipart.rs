//! `multipart/form-data` 的分片装配 —— Content-Type 的 boundary 与 body 里的
//! boundary **必须是同一个字符串**，否则服务端解析失败而请求看起来成功。
//!
//! 与 `src/lib/multipart.ts` 是同一套规则的两份实现（边界字符集、转义、缺省
//! Content-Type、非法输入的处理都逐条对齐），改动两边必须同步。
//!
//! 分片结构存在 `RequestBody::content` 里，是一个分片数组的 JSON 文本：复用既有
//! 的 `content` 字段，`RequestBody` 的形状不变，`{{var}}` 模板化也照旧作用在这个
//! 字符串上（那一步由 `variable_fill` 完成，本模块拿到的是已模板化的结果）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// 文件分片没写 `contentType` 时的缺省值，与浏览器发 FormData 一致。
const DEFAULT_FILE_CONTENT_TYPE: &str = "application/octet-stream";

/// 一个分片。字段类型不对就整份反序列化失败 → 零分片，与 TS 侧逐字段校验后
/// 丢弃整份的行为一致。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MultipartPart {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) value: String,
    #[serde(default)]
    pub(crate) filename: Option<String>,
    #[serde(default)]
    pub(crate) content_type: Option<String>,
}

/// 现生成一个 boundary。RFC 2046 限长 70 且不允许出现在分片内容里，所以用
/// 固定前缀 + 哈希指纹。
///
/// **每次请求现生成**：两条发送路径（UI / MCP）各生成各的，只要各自的 body 与
/// Content-Type 用的是同一个值即可，不需要跨路径一致。
///
/// 本 crate 没有 `rand` 依赖，故用纳秒时间戳 + 进程号 + 计数器过 SipHash。
/// 边界需要的是「本次请求内不与内容撞」，不是密码学随机。
pub(crate) fn new_boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher = DefaultHasher::new();
    (nanos, std::process::id(), seq).hash(&mut hasher);
    format!("----SimplePostFormBoundary{:016x}", hasher.finish())
}

/// 带 boundary 的 Content-Type。
pub(crate) fn content_type(boundary: &str) -> String {
    format!("multipart/form-data; boundary={boundary}")
}

/// `RequestBody::content` 的 JSON 文本 → 分片数组。
///
/// **非法输入一律返回空数组**，与 TS 侧同形：不报错、不 panic。空分片仍然是一个
/// 格式合法的 multipart body（只有收尾分隔符），服务端会报「没有分片」——比发一个
/// 内容被静默改写的 body 更容易发现。
pub(crate) fn parse_parts(content: &str) -> Vec<MultipartPart> {
    serde_json::from_str::<Vec<MultipartPart>>(content).unwrap_or_default()
}

/// 头参数里的引号与反斜杠要转义，而 CR/LF 必须整个去掉——留着就是往响应头里
/// 注入一个新头，服务端看到的分片名与用户填的不是一回事。
fn escape_header_param(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// 分片自己的 Content-Type：写了就用，没写且是文件分片则回落到二进制流类型。
fn part_content_type(part: &MultipartPart) -> Option<&str> {
    if let Some(ct) = part.content_type.as_deref().filter(|s| !s.is_empty()) {
        return Some(ct);
    }
    if part.filename.as_deref().is_some_and(|f| !f.is_empty()) {
        return Some(DEFAULT_FILE_CONTENT_TYPE);
    }
    None
}

/// 分片数组 + boundary → 真正写进 socket 的 body。行尾是 CRLF（RFC 2046）。
///
/// 零个分片得到 `--<boundary>--\r\n`，仍是格式合法的 multipart。
pub(crate) fn build_body(parts: &[MultipartPart], boundary: &str) -> String {
    let mut out = String::new();
    for part in parts {
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\nContent-Disposition: form-data; name=\"");
        out.push_str(&escape_header_param(&part.name));
        out.push('"');
        if let Some(filename) = part.filename.as_deref().filter(|f| !f.is_empty()) {
            out.push_str("; filename=\"");
            out.push_str(&escape_header_param(filename));
            out.push('"');
        }
        out.push_str("\r\n");

        if let Some(ct) = part_content_type(part) {
            out.push_str("Content-Type: ");
            out.push_str(ct);
            out.push_str("\r\n");
        }

        out.push_str("\r\n");
        out.push_str(&part.value);
        out.push_str("\r\n");
    }
    out.push_str("--");
    out.push_str(boundary);
    out.push_str("--\r\n");
    out
}

/// 把一个已解析的 `RequestBody::content`（分片数组的 JSON 文本）装到请求上，
/// 返回真正写进 socket 的 body，同时就地改写 `headers` 里的 Content-Type。
///
/// **multipart 是「用户值优先」的唯一例外。** 其余模式的 body 是用户手写的，用户
/// 自设的 Content-Type 有机会描述它；multipart 的 body 是这里用 boundary 框出来的，
/// boundary 是生成物的一部分，用户那条值在结构上描述不了它 —— 留着就是「存得对、
/// 发得错」：请求发得出去、服务端解析不出分片，而用户看不出任何异常。
///
/// 覆盖的是**值**，header 名保留用户的大小写：HTTP header 名不区分大小写，换掉它
/// 没有收益。
pub(crate) fn apply(headers: &mut Vec<(String, String)>, content: &str) -> String {
    let boundary = new_boundary();
    let ct = content_type(&boundary);
    match headers
        .iter()
        .position(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        Some(i) => headers[i].1 = ct,
        None => headers.push(("Content-Type".to_string(), ct)),
    }
    build_body(&parse_parts(content), &boundary)
}

#[cfg(test)]
#[path = "multipart_test.rs"]
mod tests;
