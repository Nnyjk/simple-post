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
        // 试着按 JSON 解析，失败则当字符串。与 Tauri 命令的 `serde_json::Value` 往返一致，
        // 故浏览器看到的是带类型的值而非带引号的字符串。
        let parsed: Value = serde_json::from_str(&v).unwrap_or(Value::String(v));
        out.insert(k, parsed);
    }
    Ok(Json(Value::Object(out)))
}

pub(crate) async fn preferences_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<PreferencesPutBody>,
) -> Result<StatusCode, HttpErrorResponse> {
    // 按 `db::preferences::set` 同样的方式重新编码该值（serde_json::to_string），
    // 使 `mcp_auto_start` 等保持一致的带类型值语义。
    let raw = serde_json::to_string(&body.value)
        .map_err(|e| map_app_error(AppError::Validation(e.to_string())))?;
    db::with_tx(&s.pool, |c| db::preferences::set(c, &body.key, &raw)).map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}
