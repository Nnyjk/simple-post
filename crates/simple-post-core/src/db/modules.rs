//! CRUD for the `modules` table.
//!
//! A module is the top-level grouping under a project (e.g. "用户中心",
//! "订单"). Collections live under modules; the chain is
//! project → module → collection → endpoint.

use rusqlite::{params, Connection, Row};

use crate::domain::{Module, ModulePatch};
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All modules belonging to a project, ordered by `sort_order` then by
/// `id` (so a fresh import is deterministic).
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<Module>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, description, sort_order \
         FROM modules \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_module)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// All modules across every project — used by the REST `GET /api/modules`
/// when no `projectId` filter is supplied (SPA bootstrap path).
pub fn list_all(conn: &mut Connection) -> Result<Vec<Module>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, description, sort_order \
         FROM modules \
         ORDER BY project_id ASC, sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![], row_to_module)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// Single module lookup. Returns `NotFound` if absent.
pub fn get(conn: &mut Connection, id: &str) -> Result<Module, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, description, sort_order \
         FROM modules WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_module(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("module {id}"))),
    }
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

pub fn create(conn: &mut Connection, m: &Module) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO modules (id, project_id, name, description, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![m.id, m.project_id, m.name, m.description, m.sort_order],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: ModulePatch,
) -> Result<Module, AppError> {
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
    if let Some(s) = patch.sort_order {
        sets.push(format!("sort_order = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Integer(s));
    }

    if sets.is_empty() {
        // No-op patch: just re-read.
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE modules SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("module {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM modules WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("module {id}")));
    }
    Ok(())
}

/// Reorder a module within its project. `target_index` is the
/// zero-based position in the post-move list.
///
/// Algorithm:
///   1. Look up the moving module's `project_id` (so we know what
///      to scope the surrounding list to).
///   2. Pull every sibling in `(sort_order, id)` order.
///   3. Remove the moving id, insert it at `target_index` (clamped
///      to `[0, len]`), then renumber `sort_order = 0..N` for all
///      siblings.
///
/// MUST be called inside `db::with_tx` — the renumber is multiple
/// UPDATEs against rows that might also be read concurrently, and
/// the caller wants a consistent list for the renderer.
///
/// Returns `NotFound` if the module id doesn't exist.
pub fn r#move(
    conn: &mut Connection,
    module_id: &str,
    target_index: i32,
) -> Result<(), AppError> {
    // 1. Find the moving module's project. Short-circuit on missing id.
    let project_id: Option<String> = conn
        .query_row(
            "SELECT project_id FROM modules WHERE id = ?1",
            params![module_id],
            |r| r.get(0),
        )
        .ok();
    let project_id = match project_id {
        Some(p) => p,
        None => return Err(AppError::NotFound(format!("module {module_id}"))),
    };

    // 2. List siblings in display order. Stable order by `(sort_order, id)`
    //    so the renumber is deterministic even when sort_order ties.
    let mut stmt = conn.prepare(
        "SELECT id FROM modules \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let mut ids: Vec<String> = stmt
        .query_map(params![project_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 3. Remove the moving id from the current position.
    ids.retain(|id| id != module_id);

    // 4. Clamp target_index into `[0, ids.len()]` so a too-large
    //    value lands at the end and a negative value lands at the
    //    front — same semantics as the renderer's drop target.
    let mut idx = if target_index < 0 { 0 } else { target_index as usize };
    if idx > ids.len() {
        idx = ids.len();
    }

    // 5. Insert at the target position.
    ids.insert(idx, module_id.to_string());

    // 6. Renumber sort_order contiguously. One UPDATE per row keeps
    //    the SQL trivially obvious; a single CASE-WHEN rewrite would
    //    be slightly faster but harder to reason about.
    let mut update = conn.prepare(
        "UPDATE modules SET sort_order = ?1 WHERE id = ?2",
    )?;
    for (i, id) in ids.iter().enumerate() {
        update.execute(params![i as i64, id])?;
    }
    drop(update);

    Ok(())
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_module(row: &Row<'_>) -> Result<Module, rusqlite::Error> {
    Ok(Module {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        sort_order: row.get(4)?,
        expanded: None, // UI-only, not persisted
    })
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
            "simple-post-modules-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        db::init(&dir).expect("init")
    }

    fn sample_module(id: &str, project_id: &str, name: &str) -> Module {
        Module {
            id: id.into(),
            project_id: project_id.into(),
            name: name.into(),
            description: None,
            sort_order: 0,
            expanded: None,
        }
    }

    #[test]
    fn create_and_list() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            create(c, &sample_module("m_new", "p_demo", "New"))?;
            Ok(())
        })
        .unwrap();
        let ms = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
        // 3 seeded modules (m_auth, m_order, m_pay) under p_demo + new
        assert!(ms.iter().any(|m| m.id == "m_new"));
        assert!(ms.iter().any(|m| m.id == "m_auth"));
    }

    #[test]
    fn update_renames_a_module() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            create(c, &sample_module("m1", "p_demo", "Old"))?;
            update(
                c,
                "m1",
                ModulePatch {
                    name: Some("New".into()),
                    ..Default::default()
                },
            )?;
            Ok(())
        })
        .unwrap();
        let m = db::with_conn(&pool, |c| get(c, "m1")).unwrap();
        assert_eq!(m.name, "New");
    }

    #[test]
    fn delete_cascades_to_collections_and_endpoints() {
        let pool = fresh_pool();
        // p_demo → m_auth → c_login → e_login_sms is the seed chain.
        let before: i64 = db::with_conn(&pool, |c| {
            Ok(c.query_row("SELECT COUNT(*) FROM endpoints", [], |r| r.get(0))?)
        })
        .unwrap();
        assert!(before > 0);

        db::with_tx(&pool, |c| delete(c, "m_auth")).unwrap();

        let after: i64 = db::with_conn(&pool, |c| {
            Ok(c.query_row("SELECT COUNT(*) FROM endpoints", [], |r| r.get(0))?)
        })
        .unwrap();
        // 4 endpoints lived under m_auth: e_login_sms, e_login_pwd, e_user_get, e_user_update
        assert_eq!(after, before - 4);
    }
}