//! CRUD for the `projects` table.
//!
//! The delete path relies on `ON DELETE CASCADE` from schema.sql —
//! dropping a project transitively removes its modules, collections,
//! endpoints, environments, base_url_definitions, and the agent
//! permission row, so the FKs stay clean without an explicit
//! cleanup loop.
//!
//! `Project.base_url_definitions` lives in the `base_url_definitions`
//! table (separate so we get FK cascade + its own indexes). Reads
//! populate it via a second query keyed by project_id.

use rusqlite::{params, Connection, Row};

use crate::domain::{Project, ProjectPatch};
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All projects with their `base_url_definitions` populated. Two
/// queries — one for the project rows, one for all defs grouped by
/// project_id — so the cost stays O(1) regardless of how many defs
/// each project has.
pub fn list(conn: &mut Connection) -> Result<Vec<Project>, AppError> {
    // Collect projects first so the `stmt` borrow on `conn` is dropped
    // before we open a second statement for the defs join.
    let mut projects = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, default_base_url_definition_id, color, \
                    created_at, updated_at \
             FROM projects \
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], row_to_project)?;
        for r in rows {
            projects.push(r.map_err(AppError::Sqlite)?);
        }
    }
    populate_defs(conn, &mut projects)?;
    Ok(projects)
}

/// Single project lookup with its `base_url_definitions` populated.
/// Returns `AppError::NotFound` if the id is absent.
pub fn get(conn: &mut Connection, id: &str) -> Result<Project, AppError> {
    let project = {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, default_base_url_definition_id, color, \
                    created_at, updated_at \
             FROM projects WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => row_to_project(row).map_err(AppError::Sqlite)?,
            None => return Err(AppError::NotFound(format!("project {id}"))),
        }
    }; // stmt + rows dropped here, releasing the immutable borrow on `conn`.
    let mut single = vec![project];
    populate_defs(conn, &mut single)?;
    Ok(single.remove(0))
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

/// Insert a project. `created_at` / `updated_at` are taken from the
/// caller-supplied struct; we don't `now()` server-side because the
/// renderer sometimes seeds optimistic timestamps for offline-first
/// flows.
///
/// Note: the `base_url_definitions` field on `Project` is intentionally
/// ignored here — base URL defs live in their own table and must be
/// inserted via `base_url_defs::create`. Callers wanting to create a
/// project with defs should run both inserts inside one transaction.
pub fn create(conn: &mut Connection, p: &Project) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO projects \
         (id, name, description, default_base_url_definition_id, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            p.id,
            p.name,
            p.description,
            p.default_base_url_definition_id,
            p.color,
            p.created_at,
            p.updated_at,
        ],
    )?;
    Ok(())
}

/// Apply a partial update. Only fields with `Some(...)` are written;
/// `Option<Option<T>>` (`description`, `base_url`,
/// `default_base_url_definition_id`) distinguishes "leave alone"
/// (outer `None`) from "clear" (inner `None`).
///
/// Returns the post-update row so the caller can echo it back to
/// the renderer without an extra `get`.
pub fn update(conn: &mut Connection, id: &str, patch: ProjectPatch) -> Result<Project, AppError> {
    // Collect owned bindings (so we can hold them in a Vec past the
    // point where `patch` borrows them) and a parallel list of the
    // SET clauses. The actual SQL is assembled at the end.
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(name.clone()));
    }
    if let Some(d) = &patch.description {
        sets.push(format!("description = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(b) = &patch.base_url {
        sets.push(format!("base_url = ?{}", binds.len() + 1));
        binds.push(match b {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(d) = &patch.default_base_url_definition_id {
        sets.push(format!(
            "default_base_url_definition_id = ?{}",
            binds.len() + 1
        ));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(c) = &patch.color {
        sets.push(format!("color = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(c.clone()));
    }
    // Always bump updated_at.
    sets.push(format!("updated_at = ?{}", binds.len() + 1));
    binds.push(rusqlite::types::Value::Integer(crate::domain::now_ms()));

    let sql = format!(
        "UPDATE projects SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));

    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    get(conn, id)
}

/// Delete a project. The cascade in the schema drops modules,
/// collections, endpoints, envs, base_url_defs and the permission
/// row in one go. Returns `NotFound` if the id is absent so the caller
/// can decide whether a silent-OK vs 404 is right.
pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_project(row: &Row<'_>) -> Result<Project, rusqlite::Error> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        base_url: None, // not persisted on the row; legacy migration only
        base_url_definitions: Vec::new(), // populated by populate_defs
        default_base_url_definition_id: row.get(3)?,
        color: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// For each project in the input vector, populate its
/// `base_url_definitions` from the join table. Two queries total
/// regardless of project count: one for all defs, then a hashmap
/// join in Rust.
fn populate_defs(conn: &mut Connection, projects: &mut [Project]) -> Result<(), AppError> {
    if projects.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, description \
         FROM base_url_definitions \
         WHERE project_id IN (SELECT id FROM projects) \
         ORDER BY project_id ASC, sort_order ASC, id ASC",
    )?;
    // Bucket defs by project_id. Small dataset (dozens of defs total),
    // so a HashMap is overkill, but it keeps the code path obvious.
    let mut by_project: std::collections::HashMap<String, Vec<crate::domain::BaseUrlDefinition>> =
        std::collections::HashMap::new();
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let project_id: String = row.get(1)?;
        let name: String = row.get(2)?;
        let description: Option<String> = row.get(3)?;
        Ok((
            project_id,
            crate::domain::BaseUrlDefinition {
                id,
                name,
                description,
            },
        ))
    })?;
    for r in rows {
        let (pid, def) = r?;
        by_project.entry(pid).or_default().push(def);
    }
    for p in projects.iter_mut() {
        if let Some(defs) = by_project.remove(&p.id) {
            p.base_url_definitions = defs;
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "projects_test.rs"]
mod tests;
