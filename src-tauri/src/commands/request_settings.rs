// ===========================================================================
// region: request_settings
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::RequestSettingsPatch;

/// Preferences KV key under which the singleton `RequestSettings` row is
/// stored. Matches the JSON-encoded value via `serde_json`.
const REQUEST_SETTINGS_KEY: &str = "request_settings";

/// App-wide request defaults (timeout, redirects, max body size). The
/// DB stores these as a JSON-encoded value in the `preferences` table
/// under the `REQUEST_SETTINGS_KEY` row. The `get` command always
/// returns a row, falling back to `RequestSettings::default()` if the
/// row is missing.
#[tauri::command]
pub fn request_settings_get(state: State<DbPool>) -> Result<RequestSettings, AppError> {
    let stored: Option<RequestSettings> = crate::db::with_conn(&state, |c| {
        crate::db::preferences::get(c, REQUEST_SETTINGS_KEY)
    })?;
    Ok(stored.unwrap_or_default())
}

#[tauri::command]
pub fn request_settings_update(
    state: State<DbPool>,
    patch: RequestSettingsPatch,
) -> Result<RequestSettings, AppError> {
    crate::db::with_tx(&state, |c| {
        let mut settings: RequestSettings =
            crate::db::preferences::get(c, REQUEST_SETTINGS_KEY)?.unwrap_or_default();
        if let Some(t) = patch.timeout_ms {
            settings.timeout_ms = t;
        }
        if let Some(f) = patch.follow_redirects {
            settings.follow_redirects = f;
        }
        if let Some(m) = patch.max_response_size_kb {
            settings.max_response_size_kb = m;
        }
        crate::db::preferences::set(c, REQUEST_SETTINGS_KEY, &settings)?;
        Ok(settings)
    })
}
