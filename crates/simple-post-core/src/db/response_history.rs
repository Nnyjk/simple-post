//! CRUD for the `response_history` table — one persisted HTTP response
//! per row, capped per endpoint.
//!
//! This table is what makes the response-history strip survive a
//! restart. It used to be a renderer-only
//! `Record<endpointId, ApiResponse[]>` that a relaunch, a project switch
//! or a closed tab erased, and nothing in the database knew a response
//! had ever happened.
//!
//! Three properties are enforced here rather than trusted to callers,
//! because all three are things a caller gets wrong silently:
//!
//!   * **The per-endpoint cap.** [`append`] trims to
//!     [`RESPONSE_HISTORY_PER_ENDPOINT`] in the same transaction as the
//!     insert, so a client that loops sends forever still leaves ten
//!     rows. Leaving this to the renderer would mean every *other*
//!     writer — a future MCP tool, a script — could grow the store
//!     without limit, and the cap would live in the one place nobody
//!     reads when debugging.
//!   * **The body cap.** [`append`] clips
//!     `RESPONSE_HISTORY_MAX_BODY_BYTES` off `body` and records it.
//!     Rejecting the response instead would lose the status line, the
//!     headers and the timing, which are the parts a history row is
//!     mostly for.
//!   * **Cleanup.** There is no `delete_for_*` here on purpose:
//!     `endpoint_id` cascades, and `nodes.parent_id` / `nodes.root_id`
//!     cascade in turn, so deleting an endpoint — or any ancestor up to
//!     the project — takes its history with it in the same DELETE. An
//!     application-level prune would be a second copy of a guarantee
//!     the foreign keys already make.
//!
//! Every function takes `&mut Connection` so the caller decides whether
//! the call sits inside a transaction. [`append`] needs one to make the
//! insert and the trim atomic.

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

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// One endpoint's history, newest first.
///
/// Bounded by the cap [`append`] maintains, so this cannot return an
/// unbounded list for a live endpoint — but an endpoint that had a
/// larger cap in an older build, or one written by a build where the cap
/// differed, can still hold more. `LIMIT` is here so the read is
/// bounded by the same constant the write is, whatever the file holds.
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

/// Every endpoint's history under one project, newest first within each
/// endpoint.
///
/// This is the bootstrap read: the renderer holds one
/// `Record<endpointId, …>` and a project switch replaces it wholesale,
/// so one round-trip is the difference between a usable history and one
/// call per endpoint. It joins through `nodes` to use the `root_id`
/// index rather than resolving ancestors in the renderer.
pub fn list_by_root(
    conn: &mut Connection,
    root_id: &str,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM response_history rh \
         JOIN nodes n ON n.id = rh.endpoint_id \
         WHERE n.root_id = ?1 \
         ORDER BY rh.endpoint_id ASC, rh.created_at DESC, rh.rowid DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

/// Store one response and trim the endpoint back to the cap.
///
/// Returns the row as it landed, including the body clip, so a caller
/// that cares can show it without a second read.
///
/// The trim is `DELETE … WHERE id NOT IN (SELECT id … LIMIT cap)`
/// rather than "count, then delete the oldest N": that is one statement,
/// it cannot be wrong about how many rows there were, and it does not
/// have a window between counting and deleting for a concurrent append
/// to slip through. SQLite serialises writers, and the whole thing runs
/// inside the caller's `BEGIN IMMEDIATE`.
///
/// `created_at` is taken once here and shared by the insert, so the
/// stored timestamp is the same value the caller would have computed.
pub fn append(
    conn: &mut Connection,
    endpoint_id: &str,
    response: &StoredResponse,
) -> Result<ResponseHistoryEntry, AppError> {
    let (body, truncated) = clip_body(&response.body);
    // A clipped body cannot be pretty-printed, so the parsed copy goes
    // too — leaving `bodyJson` populated from the full document while
    // `body` shows half of it is the one combination that shows the
    // viewer two different answers to "what did this return".
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

/// Drop everything beyond the cap, oldest first.
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

/// Drop one endpoint's history. Returns how many rows went.
///
/// Nothing calls this from a delete path — the foreign key already
/// covers that — so its only job is the explicit "clear this endpoint's
/// history" affordance, where the endpoint is alive and its rows are
/// not wanted.
pub fn clear(conn: &mut Connection, endpoint_id: &str) -> Result<usize, AppError> {
    Ok(conn.execute(
        "DELETE FROM response_history WHERE endpoint_id = ?1",
        params![endpoint_id],
    )?)
}

// -------------------------------------------------------------------------
// Internals.
// -------------------------------------------------------------------------

/// Clip `body` to the cap, on a UTF-8 boundary.
///
/// Char-boundary rather than byte-boundary because the clipped half is
/// shown in the response viewer: a `String` that stopped mid-codepoint
/// would serialise fine and then render as U+FFFD on arrival, turning a
/// deliberate truncation marker into mojibake. `char_indices` is a
/// single pass and the truncation path is the rare one, so paying for
/// the scan only when the body is actually over the cap is the right
/// trade.
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

    // Same re-wrap as `endpoint_details::row_to_detail`: a stored verb
    // the parser rejects is a `domain` failure, not a row-mapping one,
    // so it is converted into the `rusqlite::Error` this mapper owes the
    // caller — with the column named so the message is actionable.
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

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "response_history_test.rs"]
mod tests;
