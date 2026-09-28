//! Unified application error type.
//!
//! Every fallible operation in the backend returns `Result<T, AppError>`.
//! The serde impl below shapes `AppError` into a stable `{code, message}`
//! object so the React renderer can dispatch on `error.code` without
//! needing string parsing or bespoke error classes.

use serde::{Serialize, Serializer};

/// All errors that can cross the Rust ↔ JS boundary.
///
/// Each variant has a stable `code` string on the wire (the variant
/// name in snake_case) plus a human-readable `message`. New variants
/// should keep the `code` lowercase and stable across releases —
/// the frontend dispatches on it.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Resource lookup returned nothing (e.g. `db::projects::get` on a
    /// missing id). The renderer maps this to a 404 toast.
    #[error("not found: {0}")]
    NotFound(String),

    /// Caller-supplied input failed validation (empty name, bad id, …).
    /// Surfaces to the user as an inline form error.
    #[error("validation: {0}")]
    Validation(String),

    /// The agent / MCP caller is not permitted to perform the action.
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// SQLite / r2d2 / serde / io failure. The inner message is what
    /// ends up in `tracing::error!`; the wire shape stays generic so
    /// we don't leak SQL fragments into the renderer.
    #[error("db error: {0}")]
    Db(String),

    /// Network / transport failure that came up below the response
    /// layer (DNS, connect, TLS, timeout, …).
    #[error("http error: {0}")]
    Http(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("pool error: {0}")]
    Pool(#[from] r2d2::Error),
}

impl AppError {
    /// Short, stable code string used by the renderer to switch on the
    /// error category. Add new variants here AND in the match below
    /// when introducing them — otherwise the wire shape silently
    /// changes.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation",
            AppError::PermissionDenied(_) => "permission_denied",
            AppError::Db(_) => "db",
            AppError::Http(_) => "http",
            AppError::Io(_) => "io",
            AppError::Serde(_) => "serde",
            AppError::Sqlite(_) => "sqlite",
            AppError::Pool(_) => "pool",
        }
    }
}

/// Wire shape: `{ "code": "not_found", "message": "..." }`. Always
/// camelCased via the `rename_all` below so the renderer doesn't need
/// to know about Rust naming conventions.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

// -------------------------------------------------------------------------
// Tests — wire shape stability.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "error_test.rs"]
mod tests;
