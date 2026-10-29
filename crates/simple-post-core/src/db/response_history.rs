//! `response_history` 表的 CRUD——每行一条已持久化的 HTTP 响应，按 endpoint 封顶。
//!
//! 这张表让响应历史条带能挺过重启。四条性质在此强制而非交给调用方，因为四者都是调用方会
//! 静默搞错的东西：
//!
//!   * **每 endpoint 封顶。** [`append`] 在与插入同一个事务里裁到「最新的 N 条未收藏
//!     历史」，故一个无限循环发送的客户端最终也只留那么多行。交给渲染层就意味着每个
//!     **其他**写入方——将来的 MCP 工具、脚本——都能无上限地撑大库，而那个上限会住在
//!     唯一没人调试时会去读的地方。
//!   * **收藏的行不参与裁剪。** 用户显式标出的那一行不该被自动丢掉，故裁剪语句只碰
//!     `is_favorite = 0` 的行。
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
    now_ms, parse_http_method, uid, ResponseHistoryEntry, StoredRequestSnapshot, StoredResponse,
    RESPONSE_HISTORY_MAX_BODY_BYTES,
};
use crate::error::AppError;

use super::json_col_err;

const COLUMNS: &str = "rh.id, rh.endpoint_id, rh.status, rh.status_text, rh.duration_ms, \
                       rh.size_bytes, rh.content_type, rh.headers_json, rh.body, rh.body_json, \
                       rh.error, rh.method, rh.url, rh.body_truncated, rh.created_at, \
                       rh.is_favorite, rh.request_json";

// 读。

/// 一个 endpoint 的历史，最新的在前。
///
/// 返回的是「全部收藏行 + 最新的 N 条未收藏行」，N 由 [`keep_count`] 读出。受 [`append`]
/// 维持的裁剪约束，故未收藏的部分不可能返回无界的列表——但收藏的行可以无限多，因为用户
/// 说它们永久保存。
pub fn list_by_endpoint(
    conn: &mut Connection,
    endpoint_id: &str,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    let keep = keep_count(conn)?;
    let sql = format!(
        "SELECT {COLUMNS} FROM response_history rh \
         WHERE rh.endpoint_id = ?1 \
           AND ( rh.is_favorite = 1 \
                 OR rh.rowid IN ( \
                      SELECT rowid FROM response_history \
                      WHERE endpoint_id = ?1 AND is_favorite = 0 \
                      ORDER BY created_at DESC, rowid DESC \
                      LIMIT ?2 ) ) \
         ORDER BY rh.created_at DESC, rh.rowid DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![endpoint_id, keep], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

// 写。

/// 存下一条响应，并把该 endpoint 的未收藏历史裁回上限。
/// 返回落库后的行（含响应体裁剪），关心的调用方无需再读一次即可展示。
/// 裁剪是 `DELETE … WHERE rowid NOT IN (SELECT rowid … LIMIT cap)` 而非「先计数、再删最旧的 N」：
/// 那是一条语句，它不会在「到底有几行」这件事上出错，也没有计数与删除之间的窗口留给并发
/// append 钻进来。SQLite 串行化写入，且整件事跑在调用方的 `BEGIN IMMEDIATE` 之内。
/// `created_at` 在此取一次、由插入共用，故存下的时间戳与调用方本会算出的值相同。
pub fn append(
    conn: &mut Connection,
    endpoint_id: &str,
    response: &StoredResponse,
    request: Option<&StoredRequestSnapshot>,
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
    let request_json = match request {
        Some(r) => Some(serde_json::to_string(r)?),
        None => None,
    };

    conn.execute(
        "INSERT INTO response_history \
         (id, endpoint_id, status, status_text, duration_ms, size_bytes, content_type, \
          headers_json, body, body_json, error, method, url, body_truncated, created_at, \
          is_favorite, request_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 0, ?16)",
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
            request_json,
        ],
    )?;

    let keep = keep_count(conn)?;
    trim_to_cap(conn, endpoint_id, keep)?;

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
        is_favorite: false,
        request: request.cloned(),
    })
}

/// 丢掉超出上限的未收藏行，最旧的先丢。收藏的行一律不动。
fn trim_to_cap(conn: &mut Connection, endpoint_id: &str, keep: i64) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM response_history \
         WHERE endpoint_id = ?1 \
           AND is_favorite = 0 \
           AND rowid NOT IN ( \
             SELECT rowid FROM response_history \
             WHERE endpoint_id = ?1 AND is_favorite = 0 \
             ORDER BY created_at DESC, rowid DESC \
             LIMIT ?2 \
           )",
        params![endpoint_id, keep],
    )?;
    Ok(())
}

/// 按新的保留条数裁所有 endpoint 的未收藏行，收藏的行不动。
///
/// 改全局条数后调用：条数变小时历史要立刻变短，不能等下一次发送。
///
/// 逐个 endpoint 执行一条 DELETE：跨 endpoint 分组取前 N 行的单条 SQL 难以验证。
pub fn trim_all(conn: &mut Connection, keep: i64) -> Result<(), AppError> {
    let endpoint_ids = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT endpoint_id FROM response_history WHERE is_favorite = 0")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(AppError::Sqlite)?);
        }
        out
    };
    for id in &endpoint_ids {
        trim_to_cap(conn, id, keep)?;
    }
    Ok(())
}

/// 收藏或取消收藏一行。返回这一行是否存在。
///
/// 收藏状态住在行上，故它跟着那条 endpoint 走：删掉 endpoint，外键带走的正是它。
pub fn set_favorite(conn: &mut Connection, id: &str, favorite: bool) -> Result<bool, AppError> {
    let changed = conn.execute(
        "UPDATE response_history SET is_favorite = ?2 WHERE id = ?1",
        params![id, i64::from(favorite)],
    )?;
    Ok(changed > 0)
}

/// 丢掉一个 endpoint 的**未收藏**历史，返回删掉的行数。
///
/// 收藏的行留着：用户说它们永久保存，故「清空」这个动作只清自动保留的那部分。
/// 删除路径没有任何东西调用它——外键已经覆盖了——故它唯一的职责是显式的「清掉这个
/// endpoint 的历史」这个入口：此时 endpoint 还活着，只是它的行不再被需要。
pub fn clear(conn: &mut Connection, endpoint_id: &str) -> Result<usize, AppError> {
    Ok(conn.execute(
        "DELETE FROM response_history WHERE endpoint_id = ?1 AND is_favorite = 0",
        params![endpoint_id],
    )?)
}

// 内部实现。

/// 这个 endpoint 该保留多少条未收藏历史。
///
/// 条数住在 `preferences` 的 `response_history.keep` 行里，是全局设置。裁剪与读取都按它
/// 来，而不是各自带一个缺省常量——那会让「设置里改成 50」与「库里仍按 10 裁」长期各说
/// 各话。
fn keep_count(conn: &mut Connection) -> Result<i64, AppError> {
    super::preferences::response_history_keep(conn)
}

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
    let is_favorite: i64 = row.get(15)?;
    let request_json: Option<String> = row.get(16)?;

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

    let request = match request_json {
        Some(raw) => Some(serde_json::from_str(&raw).map_err(|e| json_col_err("request_json", e))?),
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
        is_favorite: is_favorite != 0,
        request,
    })
}

// 测试。

#[cfg(test)]
#[path = "response_history_test.rs"]
mod tests;
