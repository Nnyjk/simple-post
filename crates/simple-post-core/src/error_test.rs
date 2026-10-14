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
    // `r2d2::Error` 没有公开构造函数（其内部字段是私有的），故 `Pool` 变体由下面那个测试
    // 单独考，而不是在这里凭空造一个。
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

/// `r2d2::Error` 只能由该 crate 自己产生，故这里取一个真的：把唯一那条连接占在一个
/// 大小为 1 的池里，再要第二条——那必然超时。
/// 超时给得宽是刻意的：`connection_timeout` 同时管着 `build()`，而后者会急切地开一条
/// 连接，故在并行测试负载下超时的是**池的构造**，失败表现为 `build pool: Error(None)`，
/// 与被测行为无关。
/// 紧的超时在这里也换不来什么：池大小 1 且唯一连接已被借出时，第二次 `get()` 无论等多久
/// 都不可能成功。超时只决定测试多快观察到一件本已注定的事。250ms 给负载机器上打开一个
/// 内存 SQLite 留足余量，同时仍让第二次 `get()` 无条件失败。
#[test]
fn pool_code_is_distinct() {
    let mgr = r2d2_sqlite::SqliteConnectionManager::memory();
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .connection_timeout(std::time::Duration::from_millis(250))
        .build(mgr)
        .expect("build pool");

    let _held = pool.get().expect("first connection");
    let err = pool.get().expect_err("second connection must time out");

    assert_eq!(AppError::Pool(err).code(), "pool");
}
