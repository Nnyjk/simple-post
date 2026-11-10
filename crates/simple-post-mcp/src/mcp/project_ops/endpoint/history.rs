//! `endpoint_history` —— 读某个接口已记录的调用历史。
//!
//! 「接口定义」说的是这个接口**应该**返回什么，「调用历史」说的是它**实际**返回过什么：
//! 状态码、响应头、响应体，以及那次调用发出去的请求（拼接与替换之后的最终形态）。
//! 前端接入时需要的是后者——字段名、类型与取值范围只能从真实响应里读出来。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::args::EndpointHistoryArgs;
use crate::mcp::{AppState, CallToolResult};
use simple_post_core::domain::{HttpMethod, ResponseHistoryEntry, StoredRequestSnapshot};
use simple_post_core::error::AppError;

/// 默认返回几条，最新的在前。
const DEFAULT_LIMIT: usize = 5;
/// 最多返回几条。上限挡住了「一次把整个历史条读走」这种用法。
const MAX_LIMIT: usize = 50;
/// 每段 body 默认给多少字符。
const DEFAULT_BODY_CHARS: usize = 2000;
/// 每段 body 最多给多少字符。body 可以是几百 KB 的文档，故这里给一个上限。
const MAX_BODY_CHARS: usize = 20_000;

/// 读某个接口的调用历史，最新的在前。
///
/// 归属项目取自端点行的 `root_id`，故按项目那扇门查的是真实归属的那个项目。
pub(super) async fn tool_endpoint_history(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointHistoryArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_history args: {e}")))?;

    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_history").await?;

    let rows = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::response_history::list_by_endpoint(c, &a.endpoint_id)
    })?;

    // 下限是 1：`limit: 0` 回一个空数组，而空数组与「这个接口没被调用过」长得
    // 一样，调用方会据此得出错的结论。
    let limit = a
        .limit
        .map(|l| (l as usize).clamp(1, MAX_LIMIT))
        .unwrap_or(DEFAULT_LIMIT);
    let body_chars = a
        .body_chars
        .map(|c| (c as usize).min(MAX_BODY_CHARS))
        .unwrap_or(DEFAULT_BODY_CHARS);

    let entries: Vec<HistoryEntry> = rows
        .iter()
        .take(limit)
        .map(|row| HistoryEntry::from_row(row, body_chars))
        .collect();
    Ok(CallToolResult::json(entries))
}

/// 一条历史行的线形状。
///
/// 字段分成三组：这次调用是什么（时间、动词、地址）、对方回了什么（状态码、响应头、
/// 响应体）、那次发出去的请求长什么样。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryEntry {
    id: String,
    /// Unix 毫秒，由数据库打戳。
    created_at: i64,
    /// 这次调用的动词。早期的行没有存动词，此时为 `None`。
    method: Option<HttpMethod>,
    /// 最终地址：拼接、替换与追加之后的完整地址。
    url: Option<String>,
    status: u16,
    status_text: String,
    duration_ms: u64,
    size_bytes: u64,
    content_type: String,
    /// 传输层失败的原因。请求没有抵达时这里有值。
    error: Option<String>,
    /// 用户收藏了这一行。收藏的行不参与保留条数的裁剪。
    is_favorite: bool,
    /// 响应头。
    headers: BTreeMap<String, String>,
    /// 响应体。它可能是片段，见 `body_truncated`。
    body: String,
    /// 响应体是片段：为 true 时 `body` 不是完整的那一份。
    body_truncated: bool,
    /// 响应体是二进制字节，未在此返回：`body` 是空串，原始字节在库里是 base64。
    has_binary_body: bool,
    /// 那次发出去的请求。
    request: Option<RequestSnapshot>,
}

impl HistoryEntry {
    fn from_row(row: &ResponseHistoryEntry, body_chars: usize) -> Self {
        let resp = &row.response;
        let (body, clipped) = preview(&resp.body, body_chars);
        // 两个来源都会让「看到的那份」不是完整的那份：库里的 256 KiB 上限，以及这次
        // 读取自己的截短。合成一个标记，调用方不必分辨是哪一种。
        let body_truncated = row.body_truncated || clipped;
        Self {
            id: row.id.clone(),
            created_at: row.created_at,
            method: resp.method.clone(),
            url: resp.url.clone(),
            status: resp.status,
            status_text: resp.status_text.clone(),
            duration_ms: resp.duration_ms,
            size_bytes: resp.size_bytes,
            content_type: resp.content_type.clone(),
            error: resp.error.clone(),
            is_favorite: row.is_favorite,
            headers: resp.headers.clone(),
            body,
            body_truncated,
            has_binary_body: resp.body_base64.is_some(),
            request: row
                .request
                .as_ref()
                .map(|r| RequestSnapshot::from_snapshot(r, body_chars)),
        }
    }
}

/// 那次发出去的请求。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestSnapshot {
    method: HttpMethod,
    url: String,
    /// 线上那一份请求头：传输层补的那几条（`host` / `accept-encoding` 等）也在里面。
    headers: Vec<(String, String)>,
    /// 请求体。它可能是片段，见 `body_truncated`。
    body: Option<String>,
    /// 请求体是片段：为 true 时 `body` 不是完整的那一份。
    body_truncated: bool,
    /// 请求体是二进制字节，未在此返回：`body` 缺席，原始字节在库里是 base64。
    has_binary_body: bool,
}

impl RequestSnapshot {
    fn from_snapshot(r: &StoredRequestSnapshot, body_chars: usize) -> Self {
        let (body, body_truncated) = match &r.body {
            Some(text) => {
                let (clipped, truncated) = preview(text, body_chars);
                (Some(clipped), truncated)
            }
            None => (None, false),
        };
        Self {
            method: r.method.clone(),
            url: r.url.clone(),
            headers: r.headers.clone(),
            body,
            body_truncated,
            has_binary_body: r.body_base64.is_some(),
        }
    }
}

/// 取 body 的前 `max` 个字符，并说明它是不是片段。
///
/// 按**字符**切，不按字节：按字节切会落在多字节字符中间，调用方读到的是一个坏字。
fn preview(text: &str, max: usize) -> (String, bool) {
    // `nth(max)` 给的是第 `max + 1` 个字符的起点：它存在就说明文本比 `max` 长，
    // 在那里切开；它不存在说明整段都在上限之内。
    match text.char_indices().nth(max) {
        Some((idx, _)) => (text[..idx].to_string(), true),
        None => (text.to_string(), false),
    }
}
