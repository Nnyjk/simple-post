//! REST handlers for the persisted response-history strip.
//!
//! The browser transport of the three `response_history_*` Tauri
//! commands. Everything here is deliberately thin: the cap (10 rows per
//! endpoint), the 256 KiB body clip and the row mapping all live in
//! `db::response_history`, so the IPC path and this one cannot drift on
//! the two rules that are easy to get subtly wrong.
//!
//! Scope is the split worth stating once: **every** verb names a single
//! endpoint — the list because it paints one endpoint's strip, and the
//! two writes because that is the row they act on. Nothing here is
//! project-scoped: adding a `:projectId` segment would put a scope the
//! server then has to trust, and `endpoint_id` cascades off `nodes` so a
//! mismatched project would be a caller bug the write path cannot detect.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db;
use crate::domain::StoredResponse;

use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `GET /api/response-history/:endpointId` — one endpoint's rows, newest
/// first.
///
/// Shares the path with `DELETE` below, so the two verbs that act on a
/// single endpoint read the same way in a URL bar.
///
/// An id with nothing stored is `200 []`, not `404`. The commonest
/// request this route gets is the first one for an endpoint nobody has
/// sent yet, and that is an empty history rather than a missing
/// resource — answering 404 would make "brand new endpoint" look like a
/// failure on both transports.
pub(crate) async fn response_history_list(
    State(s): State<Arc<WebState>>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let rows = db::with_conn(&s.pool, |c| {
        db::response_history::list_by_endpoint(c, &endpoint_id)
    })
    .map_err(map_app_error)?;
    // `[]` rather than `null` on a serialisation failure: this row set is
    // a list the renderer appends to, and a `null` there would read as
    // "the server has no history table" rather than "the read failed".
    Ok(Json(
        serde_json::to_value(&rows).unwrap_or_else(|_| json!([])),
    ))
}

/// Body for `POST /api/response-history`.
///
/// Declared here rather than in `dto.rs` for the reason
/// `NodeDuplicateBody` gives: the whole contract of a two-field route
/// belongs next to the handler that enforces it, and there is no second
/// consumer that would justify widening `dto.rs`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseHistoryAppendBody {
    pub endpoint_id: String,
    /// The renderer's own post-adapter `ApiResponse` shape. Deserialised
    /// straight into the domain type so there is one wire contract rather
    /// than a REST DTO that has to be translated field by field.
    pub response: StoredResponse,
}

/// `POST /api/response-history` — store one response.
///
/// `201`, not `200`: the call created a row with a server-minted id and a
/// server-stamped `createdAt`, and both come back in the entry. A `200`
/// would say nothing was created while handing over a row that was.
pub(crate) async fn response_history_append(
    State(s): State<Arc<WebState>>,
    Json(body): Json<ResponseHistoryAppendBody>,
) -> Result<(StatusCode, Json<Value>), HttpErrorResponse> {
    let entry = db::with_tx(&s.pool, |c| {
        db::response_history::append(c, &body.endpoint_id, &body.response)
    })
    .map_err(map_app_error)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&entry).unwrap_or(Value::Null)),
    ))
}

/// `DELETE /api/response-history/:endpointId`
///
/// Answers `{ "removed": n }` instead of the `204` its neighbours use.
/// This is the one delete in the tree whose count the caller acts on —
/// zero means "there was nothing to clear", which is a state the strip
/// shows differently — and a `204` cannot carry it.
pub(crate) async fn response_history_clear(
    State(s): State<Arc<WebState>>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let removed = db::with_tx(&s.pool, |c| db::response_history::clear(c, &endpoint_id))
        .map_err(map_app_error)?;
    Ok(Json(json!({ "removed": removed })))
}

#[cfg(test)]
#[path = "response_history_test.rs"]
mod tests;
