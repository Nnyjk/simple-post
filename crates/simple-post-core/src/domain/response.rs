//! 响应形状：离开 Rust 的 `ApiResponse`，与已持久化的响应历史投影。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::http::HttpMethod;

// API 响应（瞬态，不落 schema）与已持久化的响应历史投影。

/// 响应离开 Rust 时的线形状，跨 Tauri IPC 传递。
///
/// - `headers` 是有序对：响应可以合法重复某个头（多行 `Set-Cookie`），map 表达不了。
/// - `src/lib/http.ts` 用 `tuplesToRecord` 折成普通对象，重复项后者胜出；这是请求路径上
///   唯一一处明知丢弃信息的地方。
/// - 适配之后的形状由 `src/types/domain.ts` 的 `ApiResponse` 描述，那是另一个类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub headers: Vec<[String; 2]>,
    pub body: String,
    /// 响应体的**二进制形态**：原始字节的 base64。
    ///
    /// 响应字节不是合法 UTF-8 时才有它（图片、压缩包、PDF…），此时 `body` 是空串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// 已持久化的响应历史

/// 单个 endpoint 保留多少条**未收藏**的响应历史。缺省 10 条。
///
/// 收藏的行不计入这个条数；用户可改这个值（见 [`crate::db::preferences::keys::RESPONSE_HISTORY_KEEP`]）。
/// 数据库侧封顶才是 `store.db` 不增长的原因，渲染器的 `MAX_HISTORY_PER_ENDPOINT` 只是镜像。
pub const RESPONSE_HISTORY_KEEP_DEFAULT: i64 = 10;

/// 单条已存响应体的硬上限（字节）。写入时裁剪，由 `ResponseHistoryEntry::body_truncated` 标记。
///
/// 历史是便利功能，不是归档：上限把每个 endpoint 的最坏情况压到约 2.5 MB。
pub const RESPONSE_HISTORY_MAX_BODY_BYTES: usize = 256 * 1024;

/// 响应在**渲染层**持有的形状，也是必须能往返出数据库的形状。
///
/// 刻意不是 [`ApiResponse`]：后者存 headers 有序对以保住重复的 `Set-Cookie`，而响应入库前已被
/// `tuplesToRecord` 折成普通对象，重复头在那时就丢了。此处存对子等于存渲染层已经拿不到的信息。
/// 本类型是 `src/types/domain.ts` 里 `ApiResponse` 的 Rust 镜像。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    /// 用 `BTreeMap` 而非 `HashMap`，行读回来时表头顺序才稳定——`HashMap` 每次读都打乱，
    /// 使两行相同的数据序列化出不同结果。
    pub headers: BTreeMap<String, String>,
    /// 已被写入路径裁剪到 [`RESPONSE_HISTORY_MAX_BODY_BYTES`]。
    pub body: String,
    /// 响应体的**二进制形态**：原始字节的 base64。字节不是合法 UTF-8 时才有它，此时 `body` 是空串。
    ///
    /// 落库时它与 `body` 分列两栏：宽松解码过的文本对二进制没有价值（每个非法字节会展开成
    /// 三个字节的 U+FFFD）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 一次发送出去的请求的快照 —— 随响应历史行一起落库。
///
/// 只存响应就看不到那次调用发出去的是什么。存的是**解析之后**的那一份：base URL 与路径已拼接、
/// 路径参数已替换、`{{var}}` 已换成取到的值、查询串已追加。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRequestSnapshot {
    pub method: HttpMethod,
    /// 最终 URL：拼接、替换与追加之后的完整地址。
    pub url: String,
    /// 有序 header 元组。HTTP 允许重名 header，map 会把它们合并，故用列表。
    ///
    /// 存的是**线上那一份**：后端在请求发出后报上来，含传输层补的 `accept` /
    /// `accept-encoding` / `host` / `content-length`。请求未抵达（超时、网络错误）时后端报不上来，
    /// 这一列存界面上拼出来的那几条。
    pub headers: Vec<(String, String)>,
    /// body 内容。没有 body 时为 `None`。
    pub body: Option<String>,
    /// body 的字节（base64）。与 `body` 互斥：二进制请求体（`binary` 模式，或带文件
    /// 分片的 multipart）走这里。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
}

/// `response_history` 的一行。
///
/// 外层而不是给 `StoredResponse` 加宽 `id` / `createdAt`：响应是载荷，这三项是行的身份。
/// `bodyTruncated` 写在这里，就不必在 `StoredResponse` 上把它设为可选。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHistoryEntry {
    pub id: String,
    pub endpoint_id: String,
    pub response: StoredResponse,
    /// `body` 已在 [`RESPONSE_HISTORY_MAX_BODY_BYTES`] 处裁剪。
    pub body_truncated: bool,
    pub created_at: i64,
    /// 用户收藏了这一行。收藏的行不参与保留条数的裁剪。
    #[serde(default)]
    pub is_favorite: bool,
    /// 那次发送出去的请求。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<StoredRequestSnapshot>,
}
