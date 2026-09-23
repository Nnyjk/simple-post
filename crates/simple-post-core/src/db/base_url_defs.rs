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
mod tests {
    use super::*;
    use crate::db;
    use crate::domain::BaseUrlDefinition;

    fn fresh_pool() -> db::DbPool {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-base-url-defs-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        db::init(&dir).expect("init")
    }

    #[test]
    fn list_by_project_returns_seeded_defs() {
        let pool = fresh_pool();
        let defs = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id, "def_default");
    }

    #[test]
    fn create_and_get() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            create(
                c,
                &BaseUrlDefinition {
                    id: "def_auth".into(),
                    name: "鉴权".into(),
                    description: Some("OAuth 入口".into()),
                },
                "p_demo",
            )?;
            Ok(())
        })
        .unwrap();
        let got = db::with_conn(&pool, |c| get(c, "def_auth")).unwrap();
        assert_eq!(got.name, "鉴权");
        assert_eq!(got.description.as_deref(), Some("OAuth 入口"));
    }

    #[test]
    fn update_renames() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            update(c, "def_default", Some("Default".into()), None)?;
            Ok(())
        })
        .unwrap();
        let got = db::with_conn(&pool, |c| get(c, "def_default")).unwrap();
        assert_eq!(got.name, "Default");
    }

    #[test]
    fn update_can_clear_description() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            update(c, "def_default", None, Some(None))?;
            Ok(())
        })
        .unwrap();
        let got = db::with_conn(&pool, |c| get(c, "def_default")).unwrap();
        assert!(got.description.is_none());
    }

    #[test]
    fn delete_removes_def() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
        let res = db::with_conn(&pool, |c| get(c, "def_default"));
        assert!(res.is_err());
    }

    #[test]
    fn set_default_updates_project_pointer() {
        let pool = fresh_pool();
        // Add a second def under p_demo, then point at it.
        db::with_tx(&pool, |c| {
            create(
                c,
                &BaseUrlDefinition {
                    id: "def_alt".into(),
                    name: "Alt".into(),
                    description: None,
                },
                "p_demo",
            )?;
            set_default(c, "p_demo", Some("def_alt"))?;
            Ok(())
        })
        .unwrap();
        let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
        assert_eq!(
            proj.default_base_url_definition_id.as_deref(),
            Some("def_alt")
        );
    }

    #[test]
    fn set_default_can_clear_with_none() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            set_default(c, "p_demo", None)?;
            Ok(())
        })
        .unwrap();
        let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
        assert!(proj.default_base_url_definition_id.is_none());
    }

    #[test]
    fn set_default_unknown_project_returns_not_found() {
        let pool = fresh_pool();
        let err =
            db::with_tx(&pool, |c| set_default(c, "p_ghost", Some("def_default"))).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }
}
