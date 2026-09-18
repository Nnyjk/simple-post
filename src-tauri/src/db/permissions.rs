//! CRUD for the `agent_permissions` table.
//!
//! Stores the per-project permission the MCP / external agent has
//! been granted. Singleton-row-per-project; absent row defaults to
//! [`Permission::ReadCall`] so a freshly imported project is usable
//! without an explicit permission grant.

use rusqlite::{params, Connection};

use crate::domain::Permission;
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// Get the permission for `project_id`. Defaults to `ReadCall` when
/// no row exists, mirroring the "be permissive until told otherwise"
/// stance the renderer applies at boot.
pub fn get(conn: &mut Connection, project_id: &str) -> Result<Permission, AppError> {
    let perm: Option<String> = conn
        .query_row(
            "SELECT permission FROM agent_permissions WHERE project_id = ?1",
            params![project_id],
            |r| r.get(0),
        )
        .ok();
    match perm {
        Some(s) => Permission::parse(&s).ok_or_else(|| {
            AppError::Db(format!("unknown permission value in DB: {s:?}"))
        }),
        None => Ok(Permission::default()),
    }
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

/// Upsert the permission. Using INSERT OR REPLACE here keeps the
/// schema simple (no UNIQUE constraint needed — `project_id` is the
/// PRIMARY KEY already).
pub fn set(conn: &mut Connection, project_id: &str, perm: Permission) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO agent_permissions (project_id, permission) \
         VALUES (?1, ?2) \
         ON CONFLICT(project_id) DO UPDATE SET permission = excluded.permission",
        params![project_id, perm.as_str()],
    )?;
    Ok(())
}

/// Remove the override for `project_id`. After this call `get` will
/// fall back to the default `ReadCall`.
pub fn clear(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM agent_permissions WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fresh_pool() -> db::DbPool {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-permissions-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        db::init(&dir).expect("init")
    }

    #[test]
    fn get_default_when_no_row() {
        let pool = fresh_pool();
        let p = db::with_conn(&pool, |c| get(c, "p_demo")).unwrap();
        assert_eq!(p, Permission::ReadCall);
    }

    #[test]
    fn set_then_get_round_trips() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, "p_demo", Permission::Full)).unwrap();
        let p = db::with_conn(&pool, |c| get(c, "p_demo")).unwrap();
        assert_eq!(p, Permission::Full);
    }

    #[test]
    fn set_overwrites_existing_value() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, "p_demo", Permission::Read)).unwrap();
        db::with_tx(&pool, |c| set(c, "p_demo", Permission::Full)).unwrap();
        assert_eq!(
            db::with_conn(&pool, |c| get(c, "p_demo")).unwrap(),
            Permission::Full
        );
    }

    #[test]
    fn clear_returns_to_default() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, "p_demo", Permission::Full)).unwrap();
        db::with_tx(&pool, |c| clear(c, "p_demo")).unwrap();
        assert_eq!(
            db::with_conn(&pool, |c| get(c, "p_demo")).unwrap(),
            Permission::ReadCall
        );
    }

    #[test]
    fn schema_rejects_unknown_permission_via_check() {
        // The schema's `CHECK(permission IN ('read','read_call','full'))`
        // is the authoritative guard — verify it actually fires.
        let pool = fresh_pool();
        let err = db::with_tx(&pool, |c| {
            c.execute(
                "INSERT INTO agent_permissions (project_id, permission) \
                 VALUES ('p_demo', 'not_a_real_perm')",
                [],
            )?;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(err.code(), "sqlite", "CHECK constraint must surface");
    }

    #[test]
    fn parse_known_values_round_trip() {
        // `Permission::parse` is the inverse of `as_str`.
        for p in [Permission::Read, Permission::ReadCall, Permission::Full] {
            assert_eq!(Permission::parse(p.as_str()), Some(p));
        }
        assert_eq!(Permission::parse("bogus"), None);
    }
}