// -------------------------------------------------------------------------
// Error → HTTP status mapping.
// -------------------------------------------------------------------------

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::error::AppError;

/// All DB / domain errors funnel through here. JSON body keeps the
/// shape stable (`{ "code": "...", "message": "..." }`) so the
/// browser can render a toast without case-matching on status.
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
    // Use the existing `code()` / `Display` so we don't duplicate the
    // string-table here.
    //
    // These keys must match `AppError::code()` in
    // `simple-post-core::error`. It previously matched `"invalid_input"`,
    // which no variant ever returns — so every validation error and every
    // permission denial fell through to 500 instead of 400 / 403, and
    // clients could not tell "you sent something wrong" from "the server
    // broke".
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
