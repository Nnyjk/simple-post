use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;

use crate::db;
use crate::error::AppError;

use super::super::{map_app_error, HttpErrorResponse, WebState};

use super::super::dto::PreferencesPutBody;

pub(crate) async fn preferences_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let pairs = db::with_conn(&s.pool, db::preferences::list_all).map_err(map_app_error)?;
    let mut out = serde_json::Map::new();
    for (k, v) in pairs {
        // Try to parse as JSON; fall back to string. This mirrors the
        // Tauri command's `serde_json::Value` round-trip so the
        // browser sees typed values, not quoted strings.
        let parsed: Value = serde_json::from_str(&v).unwrap_or(Value::String(v));
        out.insert(k, parsed);
    }
    Ok(Json(Value::Object(out)))
}

pub(crate) async fn preferences_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<PreferencesPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // Re-encode the value the same way `db::preferences::set` does
    // (serde_json::to_string), so `mcp_auto_start` etc. keep parsing
    // typed values consistently.
    let raw = serde_json::to_string(&body.value)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::preferences::set(c, &body.key, &raw)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Unix-ms timestamp of the last destructive schema rebuild, or `null`.
///
/// Mirrors the `db_recreated_at` Tauri command. It is a dedicated route
/// rather than a key inside `GET /api/preferences` because the renderer
/// needs it on every boot and the full preferences map is an internal
/// surface the browser has no other reason to read.
pub(crate) async fn db_recreated_at(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    let at: Option<i64> = db::with_conn(&s.pool, |c| {
        db::preferences::get(c, db::preferences::keys::DB_RECREATED_AT)
    })
    .map_err(map_app_error)?;
    Ok(Json(match at {
        Some(ms) => Value::Number(ms.into()),
        None => Value::Null,
    }))
}
