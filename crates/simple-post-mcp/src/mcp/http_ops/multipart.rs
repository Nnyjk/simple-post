//! `multipart/form-data` 的分片装配 —— Content-Type 的 boundary 与 body 里的
//! boundary **必须是同一个字符串**，否则服务端解析失败而请求看起来成功。
//!
//! 与 `src/lib/multipart.ts` 是同一套规则的两份实现（边界字符集、转义、缺省
//! Content-Type、非法输入的处理都逐条对齐），改动两边必须同步。
//!
//! 分片结构存在 `RequestBody::content` 里，是一个分片数组的 JSON 文本：复用既有
//! 的 `content` 字段，`RequestBody` 的形状不变，`{{var}}` 模板化由 `variable_fill`
//! 逐个分片地完成（本模块拿到的是已模板化的结果）。
//!
//! **产物是字节**：文件分片的内容是文件的字节，文本分片的内容是 UTF-8 字节。字节是
//! 合法 UTF-8 时走文本形态，否则走 base64 形态。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use simple_post_core::domain::MultipartPart;

/// 文件分片没写 `contentType` 时的缺省值，与浏览器发 FormData 一致。
const DEFAULT_FILE_CONTENT_TYPE: &str = "application/octet-stream";

/// 装配好的 multipart 请求体。
///
/// 文本形态与二进制形态**互斥**：字节是合法 UTF-8 时走文本形态，否则走 base64 形态。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AssembledBody {
    /// 文本形态。
    pub(crate) text: Option<String>,
    /// 二进制形态（base64）。
    pub(crate) base64: Option<String>,
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

/// 分片的内容字节。
///
/// 文件分片的内容是 `base64` 里的字节；解不开时给空——那份内容不是有效的 base64，
/// 而发一个内容被改写过的文件比发一个空文件更容易发现。
fn part_bytes(part: &MultipartPart) -> Vec<u8> {
    match part.base64.as_deref() {
        Some(b64) => base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .unwrap_or_default(),
        None => part.value.as_bytes().to_vec(),
    }
}

/// 分片数组 + boundary → 真正写进 socket 的 body。行尾是 CRLF（RFC 2046）。
///
/// 零个分片得到 `--<boundary>--\r\n`，仍是格式合法的 multipart。
pub(crate) fn build_body(parts: &[MultipartPart], boundary: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for part in parts {
        // 停用的分片不进 body：它还留在表格里，用户改主意时勾回来即可。
        if !part.enabled {
            continue;
        }
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        out.extend_from_slice(escape_header_param(&part.name).as_bytes());
        out.push(b'"');
        if let Some(filename) = part.filename.as_deref().filter(|f| !f.is_empty()) {
            out.extend_from_slice(b"; filename=\"");
            out.extend_from_slice(escape_header_param(filename).as_bytes());
            out.push(b'"');
        }
        out.extend_from_slice(b"\r\n");

        if let Some(ct) = part_content_type(part) {
            out.extend_from_slice(b"Content-Type: ");
            out.extend_from_slice(ct.as_bytes());
            out.extend_from_slice(b"\r\n");
        }

        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&part_bytes(part));
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");
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
pub(crate) fn apply(headers: &mut Vec<(String, String)>, content: &str) -> AssembledBody {
    let boundary = new_boundary();
    let ct = content_type(&boundary);
    match headers
        .iter()
        .position(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        Some(i) => headers[i].1 = ct,
        None => headers.push(("Content-Type".to_string(), ct)),
    }

    let bytes = build_body(
        &simple_post_core::body::parse_multipart_parts(content),
        &boundary,
    );
    match String::from_utf8(bytes) {
        Ok(text) => AssembledBody {
            text: Some(text),
            base64: None,
        },
        Err(e) => AssembledBody {
            text: None,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(e.as_bytes())),
        },
    }
}

#[cfg(test)]
#[path = "multipart_test.rs"]
mod tests;
