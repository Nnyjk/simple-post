use std::sync::Arc;

use axum::{extract::State, Json};
use serde_json::Value;

use crate::db;

use super::super::{map_app_error, HttpErrorResponse, WebState};

// ----- request settings (singleton row in preferences KV) -----------------

/// Preference key under which the singleton `RequestSettings` is
/// stored. Same key as the Tauri command — the two shapes share the
/// row.
const REQUEST_SETTINGS_KEY: &str = "request_settings";

pub(crate) async fn request_settings_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // Missing row → `unwrap_or_default()`. Matches the Tauri command's
    // "always returns a value" contract so the renderer can call this
    // unconditionally.
    let stored: Option<crate::domain::RequestSettings> =
        db::with_conn(&s.pool, |c| db::preferences::get(c, REQUEST_SETTINGS_KEY))
            .map_err(map_app_error)?;
    let settings = stored.unwrap_or_default();
    Ok(Json(serde_json::to_value(&settings).unwrap_or(Value::Null)))
}

pub(crate) async fn request_settings_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<crate::domain::RequestSettings>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // PUT semantics: full replace. Unlike the Tauri command's
    // `RequestSettingsPatch`, we accept the entire struct and write
    // it back verbatim. Callers wanting partial updates can fall
    // back to `/api/preferences` (`{"key": "request_settings", "value": ...}`).
    db::with_tx(&s.pool, |c| {
        db::preferences::set(c, REQUEST_SETTINGS_KEY, &body)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&body).unwrap_or(Value::Null)))
}
