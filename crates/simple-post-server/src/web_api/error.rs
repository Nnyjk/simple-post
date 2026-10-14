// -------------------------------------------------------------------------
// Error → HTTP 状态映射。
// -------------------------------------------------------------------------

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::error::AppError;

/// 所有 DB / domain 错误都汇到这里。JSON body 形状固定
/// （`{ "code": "...", "message": "..." }`），浏览器据此弹 toast 而不必逐个匹配状态码。
pub struct HttpErrorResponse {
    status: StatusCode,
    body: Json<Value>,
}

impl HttpErrorResponse {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: Json(json!({ "code": code, "message": message.into() })),
        }
    }
}

impl IntoResponse for HttpErrorResponse {
    fn into_response(self) -> Response {
        (self.status, self.body).into_response()
    }
}

pub fn map_app_error(e: AppError) -> HttpErrorResponse {
    // 复用既有的 `code()` / `Display`，不在这里重抄一份字符串表。
    //
    // 这些键必须与 `simple-post-core::error` 的 `AppError::code()` 一致：对不上就会
    // 让校验错误与权限拒绝一起落到 500 而不是 400 / 403，客户端便无法区分「你发错了」
    // 与「服务端坏了」。
    HttpErrorResponse::new(
        match e.code() {
            "not_found" => StatusCode::NOT_FOUND,
            "conflict" => StatusCode::CONFLICT,
            "validation" => StatusCode::BAD_REQUEST,
            "unauthorized" => StatusCode::UNAUTHORIZED,
            "permission_denied" => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
        e.code(),
        e.to_string(),
    )
}
