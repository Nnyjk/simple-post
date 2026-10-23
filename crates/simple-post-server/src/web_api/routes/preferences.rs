use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde_json::Value;

use crate::db;

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
    // 值**原样**交给 `db::preferences::set`——它自己会做一次 `serde_json::to_string`。
    //
    // 路由里若先预编码一次再交给 `set`，存下的就是 JSON 字符串（`"true"` 而不是
    // `true`）：读取端 `get::<Option<bool>>` 解析它失败，错误又被
    // `serve()` 里的 `unwrap_or(false)` 吞掉，于是 `mcp.auto_start` 永远读成
    // false——Web 形态下的「自动启动」开关写进去也等于没写。
    db::with_tx(&s.pool, |c| db::preferences::set(c, &body.key, &body.value))
        .map_err(map_app_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "preferences_test.rs"]
mod tests;
