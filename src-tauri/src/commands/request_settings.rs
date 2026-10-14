// ===========================================================================
// region: request_settings
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::RequestSettingsPatch;

/// 单例 `RequestSettings` 在 `preferences` 里的 KV key；值是 `serde_json` 编码的 JSON。
const REQUEST_SETTINGS_KEY: &str = "request_settings";

/// 缺行时回退 `RequestSettings::default()`，保证总有值可给。
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
