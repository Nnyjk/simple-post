//! `response_history` 表的 CRUD——每行一条已持久化的 HTTP 响应，按 endpoint 封顶。
//!
//! 这张表让响应历史条带能挺过重启。三条性质在此强制而非交给调用方，因为三者都是调用方会
//! 静默搞错的东西：
//!
//!   * **每 endpoint 封顶。** [`append`] 在与插入同一个事务里裁到
//!     [`RESPONSE_HISTORY_PER_ENDPOINT`]，故一个无限循环发送的客户端最终也只留十行。
//!     交给渲染层就意味着每个**其他**写入方——将来的 MCP 工具、脚本——都能无上限地撑大库，
//!     而那个上限会住在唯一没人调试时会去读的地方。
//!   * **响应体封顶。** [`append`] 把 `body` 裁到 `RESPONSE_HISTORY_MAX_BODY_BYTES` 并记录
//!     这一事实。改为拒绝整个响应会连状态行、响应头和耗时一起丢掉，而那恰恰是历史行主要
//!     为之存在的东西。
//!   * **清理。** 这里刻意没有 `delete_for_*`：`endpoint_id` 会级联，而 `nodes.parent_id` /
//!     `nodes.root_id` 又依次级联，故删掉一个 endpoint——或一路向上删到项目——会在同一条
//!     DELETE 里带走它的历史。应用层的清理只是外键已经给出的保证的第二份拷贝。
//!
//! 每个函数都取 `&mut Connection`，由调用方决定这次调用是否在事务内。[`append`] 需要事务，
//! 好让插入与裁剪是原子的。

use rusqlite::{params, Connection, Row};

use crate::domain::{
    now_ms, parse_http_method, uid, ResponseHistoryEntry, StoredResponse,
    RESPONSE_HISTORY_MAX_BODY_BYTES, RESPONSE_HISTORY_PER_ENDPOINT,
};
use crate::error::AppError;

use super::json_col_err;

const COLUMNS: &str = "rh.id, rh.endpoint_id, rh.status, rh.status_text, rh.duration_ms, \
                       rh.size_bytes, rh.content_type, rh.headers_json, rh.body, rh.body_json, \
                       rh.error, rh.method, rh.url, rh.body_truncated, rh.created_at";

// 读。

/// 一个 endpoint 的历史，最新的在前。
/// 受 [`append`] 维持的封顶约束，故活跃 endpoint 不可能返回无界的列表——但在更早构建里
/// 封顶更大的 endpoint 仍可能持有更多行。`LIMIT` 在这里，是为了让这次读与写入受同一个
/// 常量约束，无论文件里存了什么。
/// 刻意**只限一个 endpoint**。显而易见的替代方案是项目打开时一次性做一次「这个项目的一切」
/// 的读——但那是为了一个最多显示十行、且只显示用户面前那个 endpoint 的面板做一次批量拉取，
/// 且代价随项目而非随屏幕上的东西增长。用户每打开一个 endpoint 一次走索引的读，既更便宜，
/// 也正是 UI 想要的。
pub fn list_by_endpoint(
    conn: &mut Connection,
    endpoint_id: &str,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM response_history rh \
         WHERE rh.endpoint_id = ?1 \
         ORDER BY rh.created_at DESC, rh.rowid DESC \
         LIMIT {RESPONSE_HISTORY_PER_ENDPOINT}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![endpoint_id], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

// 写。

/// 存下一条响应，并把该 endpoint 裁回封顶。
/// 返回落库后的行（含响应体裁剪），关心的调用方无需再读一次即可展示。
/// 裁剪是 `DELETE … WHERE id NOT IN (SELECT id … LIMIT cap)` 而非「先计数、再删最旧的 N」：
/// 那是一条语句，它不会在「到底有几行」这件事上出错，也没有计数与删除之间的窗口留给并发
/// append 钻进来。SQLite 串行化写入，且整件事跑在调用方的 `BEGIN IMMEDIATE` 之内。
/// `created_at` 在此取一次、由插入共用，故存下的时间戳与调用方本会算出的值相同。
pub fn append(
    conn: &mut Connection,
    endpoint_id: &str,
    response: &StoredResponse,
) -> Result<ResponseHistoryEntry, AppError> {
    let (body, truncated) = clip_body(&response.body);
    // 被裁过的 body 无法再美化，故解析出的副本也一并去掉——让 `bodyJson` 仍由完整文档填充、
    // 而 `body` 只显示它的一半，是唯一会让阅读器对「这返回了什么」给出两个不同答案的组合。
    let body_json = if truncated {
        None
    } else {
        response.body_json.clone()
    };

    let id = uid();
    let created_at = now_ms();
    let headers_json = serde_json::to_string(&response.headers)?;

    conn.execute(
        "INSERT INTO response_history \
         (id, endpoint_id, status, status_text, duration_ms, size_bytes, content_type, \
          headers_json, body, body_json, error, method, url, body_truncated, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            id,
            endpoint_id,
            i64::from(response.status),
            response.status_text,
            i64::try_from(response.duration_ms).unwrap_or(i64::MAX),
            i64::try_from(response.size_bytes).unwrap_or(i64::MAX),
            response.content_type,
            headers_json,
            body,
            body_json.as_ref().map(|v| v.to_string()),
            response.error,
            response.method.as_ref().map(|m| m.as_str()),
            response.url,
            i64::from(truncated),
            created_at,
        ],
    )?;

    trim_to_cap(conn, endpoint_id)?;

    Ok(ResponseHistoryEntry {
        id,
        endpoint_id: endpoint_id.to_string(),
        response: StoredResponse {
            body,
            body_json,
            ..response.clone()
        },
        body_truncated: truncated,
        created_at,
    })
}

/// 丢掉超出封顶的部分，最旧的先丢。
fn trim_to_cap(conn: &mut Connection, endpoint_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM response_history \
         WHERE endpoint_id = ?1 \
           AND id NOT IN ( \
             SELECT id FROM response_history \
             WHERE endpoint_id = ?1 \
             ORDER BY created_at DESC, rowid DESC \
             LIMIT ?2 \
           )",
        params![endpoint_id, RESPONSE_HISTORY_PER_ENDPOINT],
    )?;
    Ok(())
}

/// 丢掉一个 endpoint 的历史，返回删掉的行数。
/// 删除路径没有任何东西调用它——外键已经覆盖了——故它唯一的职责是显式的「清掉这个
/// endpoint 的历史」这个入口：此时 endpoint 还活着，只是它的行不再被需要。
pub fn clear(conn: &mut Connection, endpoint_id: &str) -> Result<usize, AppError> {
    Ok(conn.execute(
        "DELETE FROM response_history WHERE endpoint_id = ?1",
        params![endpoint_id],
    )?)
}

// 内部实现。

/// 把 `body` 裁到封顶，落在 UTF-8 字符边界上。
/// 取字符边界而非字节边界，是因为被裁掉的那一半是要在响应阅读器里显示的：一个停在半个
/// 码位上的 `String` 序列化不会报错，到达时却渲染成 U+FFFD，把一个刻意的截断标记变成乱码。
/// `char_indices` 是单趟扫描，而截剪是罕见路径，故只在 body 真的超限时才付这次扫描的代价。
fn clip_body(body: &str) -> (String, bool) {
    if body.len() <= RESPONSE_HISTORY_MAX_BODY_BYTES {
        return (body.to_string(), false);
    }
    let end = body
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= RESPONSE_HISTORY_MAX_BODY_BYTES)
        .last()
        .unwrap_or(0);
    (body[..end].to_string(), true)
}

fn row_to_entry(row: &Row<'_>) -> Result<ResponseHistoryEntry, rusqlite::Error> {
    let headers_json: String = row.get(7)?;
    let body_json: Option<String> = row.get(9)?;
    let method_str: Option<String> = row.get(11)?;
    let body_truncated: i64 = row.get(13)?;

    // 与 `endpoint_details::row_to_detail` 同样的重新包装：一个存下来的动词若被解析器拒绝，那是
    // `domain` 的失败而非行映射的失败，故转成这个映射器欠调用方的 `rusqlite::Error`——并带上
    // 列名，好让消息可用。
    let method = match method_str {
        Some(m) => Some(parse_http_method(&m).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(super::JsonColError(format!("method: {e}"))),
            )
        })?),
        None => None,
    };

    Ok(ResponseHistoryEntry {
        id: row.get(0)?,
        endpoint_id: row.get(1)?,
        response: StoredResponse {
            status: u16::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
            status_text: row.get(3)?,
            duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
            size_bytes: row.get::<_, i64>(5)?.max(0) as u64,
            content_type: row.get(6)?,
            headers: serde_json::from_str(&headers_json)
                .map_err(|e| json_col_err("headers_json", e))?,
            body: row.get(8)?,
            body_json: body_json
                .map(|raw| serde_json::from_str(&raw).map_err(|e| json_col_err("body_json", e)))
                .transpose()?,
            error: row.get(10)?,
            method,
            url: row.get(12)?,
        },
        body_truncated: body_truncated != 0,
        created_at: row.get(14)?,
    })
}

// 测试。

#[cfg(test)]
#[path = "response_history_test.rs"]
mod tests;
