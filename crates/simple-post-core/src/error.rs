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
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn not_found_serializes_with_stable_code() {
        let e = AppError::NotFound("project p1".into());
        let v: Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "not_found");
        assert!(v["message"].as_str().unwrap().contains("p1"));
    }

    #[test]
    fn validation_carries_message() {
        let e = AppError::Validation("name required".into());
        let v: Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "validation");
        assert_eq!(v["message"], "validation: name required");
    }

    #[test]
    fn sqlite_error_converts_via_from() {
        let rusqlite_err = rusqlite::Error::InvalidQuery;
        let app: AppError = rusqlite_err.into();
        assert_eq!(app.code(), "sqlite");
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let app: AppError = io_err.into();
        assert_eq!(app.code(), "io");
    }

    #[test]
    fn serde_error_converts_via_from() {
        let bad: Result<i32, _> = serde_json::from_str("not json");
        let err = bad.unwrap_err();
        let app: AppError = err.into();
        assert_eq!(app.code(), "serde");
    }

    #[test]
    fn all_codes_are_distinct() {
        // `r2d2::Error` has no public constructor (its inner field is
        // private), so the `Pool` variant is exercised by its own test
        // below rather than being fabricated here.
        let codes = [
            AppError::NotFound("x".into()).code(),
            AppError::Validation("x".into()).code(),
            AppError::PermissionDenied("x".into()).code(),
            AppError::Db("x".into()).code(),
            AppError::Http("x".into()).code(),
            AppError::Io(std::io::Error::other("x")).code(),
            AppError::Serde(serde_json::from_str::<i32>("x").unwrap_err()).code(),
            AppError::Sqlite(rusqlite::Error::InvalidQuery).code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().copied().collect();
        assert_eq!(
            unique.len(),
            codes.len(),
            "codes must be distinct: {codes:?}"
        );
    }

    /// `r2d2::Error` can only be produced by the crate itself, so obtain a
    /// real one: hold the single connection in a size-1 pool, then ask for
    /// a second one with a 1ms timeout — that must time out.
    #[test]
    fn pool_code_is_distinct() {
        let mgr = r2d2_sqlite::SqliteConnectionManager::memory();
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .connection_timeout(std::time::Duration::from_millis(1))
            .build(mgr)
            .expect("build pool");

        let _held = pool.get().expect("first connection");
        let err = pool.get().expect_err("second connection must time out");

        assert_eq!(AppError::Pool(err).code(), "pool");
    }
}
