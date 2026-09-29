//! CRUD for the `collections` table.
//!
//! Collections can nest (via `parent_collection_id`); the constraint
//! is that every collection in a parent chain must share the same
//! `module_id` (denormalised onto the row for fast filtering). This
//! module does not enforce that constraint at write time — the
//! renderer / orchestration layer is expected to keep it. A future
//! worker can add a CHECK constraint or a trigger if it becomes
//! necessary.

use rusqlite::{params, Connection, Row};

use crate::domain::{Collection, CollectionPatch};
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All collections under a module (including nested children), ordered
/// by `sort_order` then `id`. Use this when building the tree — the
/// caller walks the parent links to construct the hierarchy.
pub fn list_by_module(conn: &mut Connection, module_id: &str) -> Result<Vec<Collection>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, module_id, parent_collection_id, name, description, sort_order \
         FROM collections \
         WHERE module_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![module_id], row_to_collection)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// All collections across every module — used by the REST
/// `GET /api/collections` when no `moduleId` filter is supplied.
pub fn list_all(conn: &mut Connection) -> Result<Vec<Collection>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, module_id, parent_collection_id, name, description, sort_order \
         FROM collections \
         ORDER BY module_id ASC, sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![], row_to_collection)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<Collection, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, module_id, parent_collection_id, name, description, sort_order \
         FROM collections WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_collection(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("collection {id}"))),
    }
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

pub fn create(conn: &mut Connection, c: &Collection) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO collections \
         (id, module_id, parent_collection_id, name, description, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            c.id,
            c.module_id,
            c.parent_collection_id,
            c.name,
            c.description,
            c.sort_order,
        ],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: CollectionPatch,
) -> Result<Collection, AppError> {
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(m) = &patch.module_id {
        sets.push(format!("module_id = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(m.clone()));
    }
    if let Some(p) = &patch.parent_collection_id {
        sets.push(format!("parent_collection_id = ?{}", binds.len() + 1));
        binds.push(match p {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(n) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(n.clone()));
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
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE collections SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("collection {id}")));
    }
    Ok(())
}

/// Move one or many collections to a new sibling position. The
/// collections are reparented to `(parent_kind, parent_id)` and
/// ordered so the first item in `collection_ids` (in caller order)
/// lands at `target_index`.
///
/// `parent_kind` is `"module"` (reparent to top-level under a module)
/// or `"collection"` (reparent to be a child of a sibling collection).
/// Unknown values yield `AppError::Validation`.
///
/// MUST be called inside `db::with_tx` — the renumber is multiple
/// UPDATEs against rows that might be read concurrently.
///
/// Semantics:
///   * The caller passes `collection_ids` in the order they should
///     appear at the destination (so a multi-select drag keeps the
///     user's pick order intact).
///   * `target_index` is clamped to `[0, sibling_len]` like
///     `modules::r#move`.
///   * Sibling collection's `sort_order` is rewritten contiguously
///     (0..N) so list reads stay cheap and unique.
pub fn move_many(
    conn: &mut Connection,
    collection_ids: &[String],
    parent_kind: &str,
    parent_id: &str,
    target_index: i32,
) -> Result<(), AppError> {
    // 1. Filter siblings: depends on parent_kind.
    let (where_clause, top_level) = match parent_kind {
        "module" => ("module_id = ?1 AND parent_collection_id IS NULL", true),
        "collection" => ("parent_collection_id = ?1", false),
        other => {
            return Err(AppError::Validation(format!(
                "unknown parent_kind: {other}"
            )));
        }
    };
    let sql = format!(
        "SELECT id FROM collections WHERE {where_clause} \
         ORDER BY sort_order ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut current: Vec<String> = stmt
        .query_map(params![parent_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 2. Remove the moving ids (preserving caller order so they
    //    re-insert at the target slot in the order the user picked).
    current.retain(|id| !collection_ids.iter().any(|m| m == id));

    // 3. Clamp target_index.
    let mut idx = if target_index < 0 {
        0
    } else {
        target_index as usize
    };
    if idx > current.len() {
        idx = current.len();
    }

    // 4. Insert in caller order starting at `idx`.
    for (offset, id) in collection_ids.iter().enumerate() {
        current.insert(idx + offset, id.clone());
    }

    // 5. Resolve the module that owns the destination. `parent_kind ==
    //    "module"` names it directly; `"collection"` means we inherit
    //    it from the target collection's denormalised `module_id`.
    let new_module_id: String = if top_level {
        parent_id.to_string()
    } else {
        conn.query_row(
            "SELECT module_id FROM collections WHERE id = ?1",
            params![parent_id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound(format!("collection {parent_id}")))?
    };

    // 6. Which of the moved rows change owner? Captured *before* step 7
    //    rewrites them — afterwards the comparison is always false.
    let mut moved_across_modules: Vec<String> = Vec::new();
    for id in collection_ids {
        let current_module: String = conn
            .query_row(
                "SELECT module_id FROM collections WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|_| AppError::NotFound(format!("collection {id}")))?;
        if current_module != new_module_id {
            moved_across_modules.push(id.clone());
        }
    }

    // 7. Rewrite sort_order AND reparent each row. Two flavours
    //    because moving under a module clears parent_collection_id
    //    while moving under a collection sets it.
    if top_level {
        let mut update = conn.prepare(
            "UPDATE collections SET sort_order = ?1, parent_collection_id = NULL, module_id = ?2 \
             WHERE id = ?3",
        )?;
        for (i, id) in current.iter().enumerate() {
            update.execute(params![i as i64, parent_id, id])?;
        }
        drop(update);
    } else {
        let mut update = conn.prepare(
            "UPDATE collections SET sort_order = ?1, parent_collection_id = ?2 \
             WHERE id = ?3",
        )?;
        for (i, id) in current.iter().enumerate() {
            update.execute(params![i as i64, parent_id, id])?;
        }
        drop(update);
    }

    // 8. Re-derive `module_id` across the moved subtrees. `module_id`
    //    is denormalised onto every collection so that
    //    `list_by_module` can return a whole subtree in one indexed
    //    read — which makes it a *derived* value. A cross-module move
    //    therefore has to re-derive it for the entire subtree, not
    //    just the rows the caller named. Skipping this leaves the
    //    store's optimistic `moduleId` fixup correct on screen and
    //    wrong on reload, and `list_by_module` stops returning the
    //    moved subtree at all.
    reassign_subtree_module(conn, &moved_across_modules, &new_module_id)?;

    Ok(())
}

/// Rewrite `module_id` for `roots` and every descendant of `roots`.
///
/// The walk uses `UNION` rather than `UNION ALL` so a pre-existing
/// cycle in the data terminates the recursion instead of looping
/// forever — a collection is allowed to be nested arbitrarily deep, and
/// nothing above this function defends against a cycle at write time.
///
/// MUST be called inside `db::with_tx` — one UPDATE per subtree node.
fn reassign_subtree_module(
    conn: &mut Connection,
    roots: &[String],
    new_module_id: &str,
) -> Result<(), AppError> {
    if roots.is_empty() {
        return Ok(());
    }
    let mut sql = String::from("WITH RECURSIVE sub(id) AS (");
    for (i, _) in roots.iter().enumerate() {
        if i > 0 {
            sql.push_str(" UNION ");
        }
        sql.push_str(&format!("SELECT id FROM collections WHERE id = ?{}", i + 1));
    }
    sql.push_str(
        " UNION SELECT c.id FROM collections c \
         JOIN sub ON c.parent_collection_id = sub.id) \
         SELECT id FROM sub",
    );

    let mut stmt = conn.prepare(&sql)?;
    let subtree: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(roots.iter()), |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut update = conn.prepare("UPDATE collections SET module_id = ?1 WHERE id = ?2")?;
    for id in &subtree {
        update.execute(params![new_module_id, id])?;
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_collection(row: &Row<'_>) -> Result<Collection, rusqlite::Error> {
    Ok(Collection {
        id: row.get(0)?,
        module_id: row.get(1)?,
        parent_collection_id: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        sort_order: row.get(5)?,
        expanded: None,
    })
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "collections_test.rs"]
mod tests;
