//! 响应形状：离开 Rust 的 `ApiResponse`，与已持久化的响应历史投影。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::http::HttpMethod;

// API 响应（瞬态，不落 schema）。`ApiResponse` 本身仍不落库；其下是已持久化的投影，
// 是个**不同的**类型，而不是复用的同一个。

/// 响应离开 Rust 时的线形状。
///
/// 它**不是**渲染层最终持有的形状，也不是进程内形状——像这里其余类型一样跨 Tauri IPC。
/// 差别在 `headers`：此侧建模为有序对，因为响应可以合法重复某个头（多行 `Set-Cookie` 之类），
/// 而 map 表达不了。渲染层要的是普通对象，故 `src/lib/http.ts` 在到达时用
/// `tuplesToRecord` 折叠——有损，且是请求路径上唯一一处明知丢弃信息的地方；重复项按
/// 后者胜出。
/// Rust 侧没有任何东西描述适配之后的形状。`src/types/domain.ts` 的 `ApiResponse` 才是那份
/// 描述，且它刻意是另一个类型，而非声称镜像本类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub headers: Vec<[String; 2]>,
    pub body: String,
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

/// 单个 endpoint 保留多少条响应。渲染器 `stores/slices/ui-response.ts` 的
/// `MAX_HISTORY_PER_ENDPOINT` 与之镜像——store 侧封顶使所展示的数组不增长，数据库侧封顶
/// 使 `store.db` 不增长。两者一致是锦上添花；真正给文件封顶的是数据库侧，故日后漏掉
/// store 侧的调用方也无法让库无限增长。
pub const RESPONSE_HISTORY_PER_ENDPOINT: i64 = 10;

/// 单条已存响应体的硬上限（字节）。
///
/// 历史是便利功能——要点是「这个 endpoint 十分钟前返回了什么」，不是归档。没有上限时，
/// 一个 endpoint 连返十次 50 MB 就把 500 MB 塞进用户的 `store.db` 且无法挽回，故写入时裁剪，
/// 并由 `ResponseHistoryEntry::body_truncated` 标记发生过。256 KiB 对人调试时读的 JSON 和
/// HTML 足够宽裕，把每 endpoint 最坏情况压到约 2.5 MB。
pub const RESPONSE_HISTORY_MAX_BODY_BYTES: usize = 256 * 1024;

/// 响应在**渲染层**持有的形状，也就是必须能往返出数据库的形状。
///
/// 刻意不是 [`ApiResponse`]：那个类型把 headers 建模为有序对，只为不丢重复的 `Set-Cookie`
/// 而熬过一次 IPC。响应入库时早已越过那道边界、并被 `tuplesToRecord` 折成普通对象——重复头的
/// 丢失已经发生，不是本类型的问题。此处存对子等于存渲染层已不再拥有的信息，而渲染层自己的
/// `ApiResponse` 就得多长一种形状。
/// 本类型是 `src/types/domain.ts` 里 `ApiResponse` 的 Rust 镜像，那同样是适配**之后**的类型，
/// 而非上面 [`ApiResponse`] 的镜像。
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

/// `response_history` 的一行。
///
/// 外面包一层而不是给已存响应加宽 `id` / `createdAt`，是因为两者生命周期不同：响应是载荷，
/// 这三个是行的身份。它也让 `bodyTruncated` 紧挨它所描述的 body——把这个标志抹到
/// `StoredResponse` 上，就得在那里把它设为可选、在这里设为必填，只为一比特的信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHistoryEntry {
    pub id: String,
    pub endpoint_id: String,
    pub response: StoredResponse,
    /// `body` 已在 [`RESPONSE_HISTORY_MAX_BODY_BYTES`] 处裁剪。
    pub body_truncated: bool,
    pub created_at: i64,
}
