//! CRUD for the `base_url_definitions` table.
//!
//! A service is owned by the **project**, and every module sees the whole
//! list — the user's rule is 「服务应该绑定到项目，这样每一个模块都会全部包含
//! 这些服务」. So there is no module filter here at all: two services in one
//! project cannot share a `name`, and an endpoint's
//! `base_url_definition_id` means the same thing wherever the endpoint
//! sits, which is what lets a drag between modules keep working without
//! re-pointing anything.
//!
//! The URL a service resolves to is not in this table. It is the
//! (environment × module × service) cell in
//! `super::environments::module_base_urls`; ★ lives on the project
//! (`projects.default_base_url_definition_id`), not on a row.
//!
//! `module_id` and `is_default` still exist as columns because a build
//! shipped with them and dropping them would mean a rebuild would mean a
//! `SCHEMA_VERSION` bump would mean `ensure_current_schema` deletes the
//! user's store. [`list_by_project`] and [`get`] deliberately do not read
//! them, and nothing here writes them.

use rusqlite::{params, Connection, Row};

use crate::domain::BaseUrlDefinition;
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All definitions for a project, ordered by `sort_order` (so the
/// picker matches the order the user originally set them in).
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<BaseUrlDefinition>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description \
         FROM base_url_definitions \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_def)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<BaseUrlDefinition, AppError> {
    let mut stmt =
        conn.prepare("SELECT id, name, description FROM base_url_definitions WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_def(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("base_url_definition {id}"))),
    }
}

/// The project a definition belongs to. Callers need this to gate a
/// mutation on the owning project — `BaseUrlDefinition` deliberately does
/// not carry `project_id`, so this is the only way to recover it from a
/// definition id alone.
pub fn project_id_of(conn: &mut Connection, id: &str) -> Result<String, AppError> {
    let pid: Option<String> = conn
        .query_row(
            "SELECT project_id FROM base_url_definitions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    match pid {
        Some(p) => Ok(p),
        None => Err(AppError::NotFound(format!("base_url_definition {id}"))),
    }
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

pub fn create(
    conn: &mut Connection,
    def: &BaseUrlDefinition,
    project_id: &str,
) -> Result<(), AppError> {
    // `sort_order` defaults to 0 on insert (per the schema). Re-numbers
    // can be applied with `reorder` if the renderer asks for it.
    conn.execute(
        "INSERT INTO base_url_definitions (id, project_id, name, description) \
         VALUES (?1, ?2, ?3, ?4)",
        params![def.id, project_id, def.name, def.description],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    name: Option<String>,
    description: Option<Option<String>>,
) -> Result<BaseUrlDefinition, AppError> {
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(n) = name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(n));
    }
    if let Some(d) = description {
        sets.push(format!("description = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v),
            None => rusqlite::types::Value::Null,
        });
    }

    if sets.is_empty() {
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE base_url_definitions SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("base_url_definition {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    // ★ points at a row here, so deleting the row has to take the pointer
    // with it. Leaving it would not crash — resolution treats an unknown
    // service as "no base URL" — but the project would keep advertising a
    // default that no longer exists, and the ★ menu would render a ghost
    // entry until someone clicked it. One `UPDATE`, no promotion: with a
    // single ★ per project there is nothing to promote it to.
    let mut tx = conn.savepoint()?;
    let removed = tx.execute(
        "DELETE FROM base_url_definitions WHERE id = ?1",
        params![id],
    )?;
    if removed == 0 {
        // Nothing matched: roll the savepoint back rather than reporting
        // success, so a typo'd id is a NotFound like every other CRUD
        // call here. The star update below would otherwise be what decides
        // it, and it matches nothing whenever another service holds the
        // star — i.e. exactly when the delete *did* have work to do.
        tx.rollback()?;
        return Err(AppError::NotFound(format!("base_url_definition {id}")));
    }
    tx.execute(
        "UPDATE projects SET default_base_url_definition_id = NULL \
         WHERE default_base_url_definition_id = ?1",
        params![id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Mark `id` as the project's ★ service. Updates
/// `projects.default_base_url_definition_id` only; envs and other
/// defs are untouched.
///
/// ★ is project-scoped, not row-scoped: the service list is shared by
/// every module, so a per-service flag would make the "default" mean
/// different things to different modules. What an endpoint naming no
/// service resolves against is the project ★, then its own module's cell
/// in the environment — see the resolution order in the settings model.
///
/// Pass `def_id = None` to clear the default.
pub fn set_default(
    conn: &mut Connection,
    project_id: &str,
    def_id: Option<&str>,
) -> Result<(), AppError> {
    let rows = conn.execute(
        "UPDATE projects SET default_base_url_definition_id = ?1 WHERE id = ?2",
        params![def_id, project_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {project_id}")));
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_def(row: &Row<'_>) -> Result<BaseUrlDefinition, rusqlite::Error> {
    Ok(BaseUrlDefinition {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
    })
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "base_url_defs_test.rs"]
mod tests;
