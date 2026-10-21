use std::sync::Arc;

use axum::{extract::State, Json};
use serde_json::Value;

use crate::db;

use super::super::{map_app_error, HttpErrorResponse, WebState};

// ----- request settings（preferences KV 里的单例行）-----------------

/// 存放 `RequestSettings` 单例的偏好键。与 Tauri 命令同一个键——两种形态共用一行。
const REQUEST_SETTINGS_KEY: &str = crate::db::preferences::keys::REQUEST_SETTINGS;

pub(crate) async fn request_settings_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // 行缺失 → `unwrap_or_default()`。与 Tauri 命令「总是返回一个值」的契约一致，
    // 故 renderer 可以无条件调它。
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
    // PUT 语义：整体替换。与 Tauri 命令的 `RequestSettingsPatch` 不同，这里收整个结构体
    // 并原样写回。想要局部更新的调用方可以退回 `/api/preferences`
    // （`{"key": "request_settings", "value": ...}`）。
    //
    // `RequestSettings` 带 `#[serde(default)]`，故省略某字段的 body 会被接受，且该字段
    // 被重置为默认值而非被拒。这是为了读取旧版本写下的 blob 而刻意做的取舍；见该字段
    // 自己的文档。要整体替换请把三个字段都发上。
    db::with_tx(&s.pool, |c| {
        db::preferences::set(c, REQUEST_SETTINGS_KEY, &body)
    })
    .map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&body).unwrap_or(Value::Null)))
}
