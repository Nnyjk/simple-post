//! CRUD for the `base_url_definitions` table.
//!
//! Each project declares a *set* of named baseUrl slots (e.g.
//! "鉴权服务", "数据服务"); each environment then provides a concrete
//! URL for each name. The definitions live in their own table so the
//! FK into `projects` carries the cascade for free.

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
    let rows = conn.execute(
        "DELETE FROM base_url_definitions WHERE id = ?1",
        params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("base_url_definition {id}")));
    }
    Ok(())
}

/// Mark `id` as the project's default base-url definition. Updates
/// `projects.default_base_url_definition_id` only; envs and other
/// defs are untouched.
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
