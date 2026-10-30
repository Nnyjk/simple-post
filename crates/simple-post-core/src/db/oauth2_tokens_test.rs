use super::*;
use crate::db;
use std::path::PathBuf;

fn fresh_pool() -> db::DbPool {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "simple-post-oauth2-tokens-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(&dir).expect("init")
}

fn token(access: &str) -> OAuth2Token {
    OAuth2Token {
        access_token: access.into(),
        refresh_token: Some("rt-1".into()),
        token_type: "Bearer".into(),
        scope: Some("read".into()),
        expires_at: Some(1_700_000_000),
        obtained_at: 1_600_000_000,
    }
}

#[test]
fn get_is_none_before_anything_is_written() {
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "nope")).unwrap();
    assert_eq!(got, None);
}

#[test]
fn upsert_then_get_round_trips_every_column() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-1"))).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "k1")).unwrap().unwrap();
    assert_eq!(got, token("at-1"));
}

#[test]
fn upsert_replaces_rather_than_duplicating() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-1"))).unwrap();
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-2"))).unwrap();
    let rows = db::with_conn(&pool, |c| {
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM oauth2_tokens WHERE auth_key = 'k1'",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    })
    .unwrap();
    assert_eq!(rows, 1, "同一份配置只能有一行，否则读到哪一行取决于 rowid");
    let got = db::with_conn(&pool, |c| get(c, "k1")).unwrap().unwrap();
    assert_eq!(got.access_token, "at-2");
}

/// `refresh_token` 为 NULL 是「确实没有」，而**不是**「本次没带」。
/// 若刷新响应不带 refresh_token 时被写成 NULL，这份配置的刷新机会就永久消失了。
#[test]
fn a_null_refresh_token_clears_the_previous_one() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-1"))).unwrap();

    let mut next = token("at-2");
    next.refresh_token = None;
    db::with_tx(&pool, |c| upsert(c, "k1", &next)).unwrap();

    let got = db::with_conn(&pool, |c| get(c, "k1")).unwrap().unwrap();
    assert_eq!(got.refresh_token, None);
    assert_eq!(got.access_token, "at-2");
}

#[test]
fn a_null_expires_at_is_kept_as_unknown_rather_than_coerced_to_zero() {
    let pool = fresh_pool();
    let mut t = token("at-1");
    t.expires_at = None;
    db::with_tx(&pool, |c| upsert(c, "k1", &t)).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "k1")).unwrap().unwrap();
    assert_eq!(
        got.expires_at, None,
        "IdP 没给 expires_in 时不能当成「1970 年就过期」"
    );
}

#[test]
fn delete_removes_only_the_named_key() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-1"))).unwrap();
    db::with_tx(&pool, |c| upsert(c, "k2", &token("at-2"))).unwrap();
    db::with_tx(&pool, |c| delete(c, "k1")).unwrap();
    assert!(db::with_conn(&pool, |c| get(c, "k1")).unwrap().is_none());
    assert!(db::with_conn(&pool, |c| get(c, "k2")).unwrap().is_some());
}

/// 既有用户的库里**没有**这张表——`schema.sql` 里是 `CREATE TABLE IF NOT EXISTS`，
/// 所以它由启动时的 `db::migrate` 白得。故重开一个已经有这张表的库不该清掉它：
/// 为这张表 bump 版本号会让 `ensure_current_schema` 删库。
#[test]
fn the_table_is_reachable_without_bumping_the_schema_version() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "simple-post-oauth2-tokens-reopen-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| upsert(c, "k1", &token("at-1"))).unwrap();
    drop(pool);

    let pool = db::init(&dir).expect("re-init");
    let got = db::with_conn(&pool, |c| get(c, "k1")).unwrap();
    assert_eq!(
        got,
        Some(token("at-1")),
        "重开一个已经有这张表的库不该清掉它"
    );
}
