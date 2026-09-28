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
