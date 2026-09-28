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
        Some(s) => Permission::parse(&s)
            .ok_or_else(|| AppError::Db(format!("unknown permission value in DB: {s:?}"))),
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
#[path = "permissions_test.rs"]
mod tests;
