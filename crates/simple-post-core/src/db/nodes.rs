//! CRUD for the `nodes` table — the merged resource tree.
//!
//! One row per node, discriminated by `type`, linked by `parent_id`,
//! with `root_id` denormalising the owning project so a whole tree is
//! one indexed read and "which project does this belong to" is a
//! primary-key hit instead of a walk up the parent chain.
//!
//! ## What the merge lost, and what replaces it
//!
//! The old three-table layout got "a parent always accepts the types
//! its column could name" for free, from foreign keys like
//! `endpoints.collection_id REFERENCES collections(id)`. One
//! polymorphic `parent_id` cannot carry that: SQLite's CHECK
//! constraints cannot read another row.
//!
//! So the parent/child matrix lives in
//! [`ResourceNodeType::may_under`] and is enforced here on every write,
//! with matching triggers in `schema.sql` as a second line of defence.
//! The trigger half is not redundant belt-and-braces: the MCP tools are
//! this project's primary external write path, and a future caller that
//! skips `db::nodes` would otherwise be free to build a tree no reader
//! can walk.
//!
//! ## `root_id` is derived, and derived values need maintenance
//!
//! It exists so that `list_by_root` is one indexed scan and
//! `root_id_of` is one primary-key lookup — the first step of the
//! per-project permission gate. The cost is that a move across a
//! project boundary has to re-derive it across the whole subtree, not
//! just the rows the caller named. [`move_many`] does that, and
//! `subtree_module_id_cascade_has_a_test` is the regression net.

use rusqlite::{params, Connection, Row};

use crate::domain::{EndpointDetail, ResourceNode, ResourceNodePatch, ResourceNodeType};
use crate::error::AppError;

const COLUMNS: &str = "id, root_id, parent_id, type, name, description, sort_order, \
                       created_at, updated_at";

/// Same list, qualified for the queries that join a second table.
const COLUMNS_N: &str = "n.id, n.root_id, n.parent_id, n.type, n.name, n.description, \
                         n.sort_order, n.created_at, n.updated_at";

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// Every node under one project, ordered for display.
///
/// This replaces the three list commands the renderer used to issue
/// without filters, which returned the whole database regardless of
/// which project was open.
pub fn list_by_root(conn: &mut Connection, root_id: &str) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM nodes WHERE root_id = ?1 \
         ORDER BY sort_order ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// Every node of one type under one project.
///
/// `resource_type` is interpolated rather than bound because
/// [`ResourceNodeType::as_str`] returns from a closed set of three
/// literals — there is no user-controlled string reaching the SQL.
pub fn list_by_root_and_type(
    conn: &mut Connection,
    root_id: &str,
    resource_type: ResourceNodeType,
) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM nodes WHERE root_id = ?1 AND type = ?2 \
         ORDER BY sort_order ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id, resource_type.as_str()], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// Every node in the database. Used by the MCP list tools, which answer
/// for a project the caller named rather than one the UI has active.
pub fn list_all(conn: &mut Connection) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM nodes ORDER BY root_id ASC, sort_order ASC, id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// Substring search over the tree.
///
/// Endpoint `url` lives in `endpoint_details`, so this joins to reach
/// it — the same reason the config was split out. A module or
/// collection has no url, which the `n.type = 'endpoint'` guard keeps
/// from matching a null column.
///
/// Must be called inside `db::with_tx` if the caller also wants the
/// matching endpoints' details; the join is read-only, so `with_conn`
/// is enough on its own.
pub fn search(conn: &mut Connection, q: &str) -> Result<Vec<ResourceNode>, AppError> {
    let pattern = format!("%{q}%");
    let sql = format!(
        "SELECT {COLUMNS_N} FROM nodes n \
         LEFT JOIN endpoint_details e ON e.node_id = n.id \
         WHERE n.name LIKE ?1 OR n.description LIKE ?1 \
            OR (n.type = 'endpoint' AND e.url LIKE ?1) \
         ORDER BY n.root_id ASC, n.sort_order ASC, n.id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![pattern], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// Copy an endpoint and its configuration under the same parent.
///
/// Endpoints only — the same shape as the old `db::endpoints::duplicate`
/// this replaces, and for the same reason: a "copy" of a module or a
/// collection would be an empty shell, and the old UI has no way to
/// express that. A subtree copy is a different feature, not a
/// duplicate.
///
/// MUST be called inside `db::with_tx` — the node and its detail row
/// have to land together or the copy cannot be opened.
///
/// `new_id` lets the caller name the copy. `None` mints one here, which
/// is what a stateless caller (the MCP tool) wants. The desktop UI
/// passes its own because it duplicates *optimistically*: it writes the
/// row into the store, activates the tab and drops the user straight
/// into a rename before this call has even returned. An id minted here
/// would be minted too late — the tab, the active selection and the
/// in-progress rename would all be pointing at a row that does not
/// exist, and the workspace rendered the tab as "接口已删除". Same
/// contract as `nodes_create`, where the SPA's id is honoured.
pub fn duplicate(
    conn: &mut Connection,
    id: &str,
    new_id: Option<&str>,
) -> Result<ResourceNode, AppError> {
    let node = get(conn, id)?;
    if node.kind != ResourceNodeType::Endpoint {
        return Err(AppError::Validation(format!(
            "only an endpoint can be duplicated; node {id} is a {}",
            node.kind.as_str()
        )));
    }
    let detail = super::endpoint_details::get(conn, id)?;
    let now = crate::domain::now_ms();
    let copy = ResourceNode {
        id: new_id
            .map(str::to_string)
            .unwrap_or_else(crate::domain::uid),
        // ` (副本)` rather than the English marker: the row's name is what
        // the user reads in a Chinese UI, and the SPA writes the same
        // suffix optimistically — the two disagreeing made the copy appear
        // to rename itself a moment after it was created.
        name: format!("{} (副本)", node.name),
        // Straight after the original. Not `MAX + 1` — the old
        // implementation pushed copies to the tail, which meant
        // duplicating an endpoint you were looking at moved it out from
        // under the cursor.
        sort_order: node.sort_order + 1,
        created_at: now,
        updated_at: now,
        ..node
    };
    let copied_detail = EndpointDetail {
        node_id: copy.id.clone(),
        ..detail
    };
    create(conn, &copy, Some(&copied_detail))?;
    get(conn, &copy.id)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<ResourceNode, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM nodes WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_node(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("node {id}"))),
    }
}

/// The project a node belongs to, in one lookup.
///
/// The old layout needed two: `collections.module_id`, then
/// `modules.project_id`. This is the first step of the per-project
/// permission gate in `db::permissions`, so it runs on every
/// mutating MCP tool.
pub fn root_id_of(conn: &mut Connection, id: &str) -> Result<String, AppError> {
    conn.query_row(
        "SELECT root_id FROM nodes WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!("node {id}")),
        other => AppError::Sqlite(other),
    })
}

/// Which module a node sits under, or `None` when it is not under one.
///
/// Every node except a module has exactly one module above it, and the
/// endpoint configuration's base URL is keyed by that module: the live
/// value layer is `moduleBaseUrls[service][module]`, so resolving an
/// endpoint's URL is two lookups — which service, then which module —
/// and this is the second. It is a tree question rather than a property
/// of the node's own row, which is why it is not denormalised the way
/// `root_id` is.
///
/// `None` is a real answer, not a failure: the node itself may be a
/// module, or not exist at all. Callers that need to tell those apart ask
/// [`type_of`] first or simply treat "no module" as "no base URL",
/// which is what resolution does. The walk skips the node itself, so a
/// module is never reported as its own answer.
pub fn module_id_of(conn: &mut Connection, id: &str) -> Result<Option<String>, AppError> {
    let found: Option<String> = conn
        .query_row(
            "WITH RECURSIVE up(id, parent_id, type, depth) AS ( \
                 SELECT id, parent_id, type, 0 FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT p.id, p.parent_id, p.type, up.depth + 1 \
                 FROM nodes p JOIN up ON p.id = up.parent_id \
             ) \
             SELECT id FROM up WHERE type = 'module' AND depth > 0 LIMIT 1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    Ok(found)
}

/// The type of a node, or `NotFound`.
pub fn type_of(conn: &mut Connection, id: &str) -> Result<ResourceNodeType, AppError> {
    let raw: String = conn
        .query_row("SELECT type FROM nodes WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!("node {id}")),
            other => AppError::Sqlite(other),
        })?;
    ResourceNodeType::parse(&raw).ok_or_else(|| {
        // The CHECK constraint should make this unreachable; surfacing it
        // beats mapping an unknown type onto some arbitrary variant.
        AppError::Db(format!("node {id} has unrecognised type {raw:?}"))
    })
}

/// `roots` plus every descendant of every root, in BFS-stable order.
///
/// Uses `UNION` rather than `UNION ALL` so a cycle already present in
/// the data terminates the walk instead of looping forever. Nothing
/// above this function defends against a cycle at write time, so this
/// has to be the one place that can.
pub fn subtree_ids(conn: &mut Connection, roots: &[String]) -> Result<Vec<String>, AppError> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = String::from("WITH RECURSIVE sub(id) AS (");
    for (i, _) in roots.iter().enumerate() {
        if i > 0 {
            sql.push_str(" UNION ");
        }
        sql.push_str(&format!("SELECT id FROM nodes WHERE id = ?{}", i + 1));
    }
    sql.push_str(
        " UNION SELECT c.id FROM nodes c \
         JOIN sub ON c.parent_id = sub.id) \
         SELECT id FROM sub",
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(roots.iter()), |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

/// Insert a node, and its `endpoint_details` row when the type calls
/// for one.
///
/// Both halves go in together: a node without its detail row, or a
/// detail row without its node, is a tree the reader cannot walk. The
/// caller supplies the `detail` for an endpoint and must pass `None`
/// otherwise — [`check_detail_agreement`] enforces that before the
/// insert, so the mistake is reported as a validation error rather
/// than surfacing later as a half-populated endpoint.
pub fn create(
    conn: &mut Connection,
    node: &ResourceNode,
    detail: Option<&EndpointDetail>,
) -> Result<ResourceNode, AppError> {
    check_detail_agreement(node, detail)?;
    validate_placement(conn, node.kind, node.parent_id.as_deref(), &node.root_id)?;

    conn.execute(
        "INSERT INTO nodes \
         (id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            node.id,
            node.root_id,
            node.parent_id,
            node.kind.as_str(),
            node.name,
            node.description,
            node.sort_order,
            node.created_at,
            node.updated_at,
        ],
    )?;
    if let Some(d) = detail {
        super::endpoint_details::create(conn, d)?;
    }
    Ok(node.clone())
}

/// Partial update of the structural columns.
///
/// `parent_id` routes through [`reparent`] rather than being written
/// directly, because moving is the operation that has to keep the type
/// matrix and the subtree's `root_id` honest — a plain UPDATE of
/// `parent_id` would do neither.
pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: ResourceNodePatch,
) -> Result<ResourceNode, AppError> {
    // Named apart from the `reparent` function it calls.
    let reparent_to = patch.parent_id.clone();

    // Built column-by-column rather than from a fixed list, because each
    // field is independently optional and the double Option on
    // `description` distinguishes "leave it" from "clear it".
    let mut named: Vec<&str> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(n) = &patch.name {
        named.push("name");
        binds.push(rusqlite::types::Value::Text(n.clone()));
    }
    if let Some(d) = &patch.description {
        named.push("description");
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(s) = patch.sort_order {
        named.push("sort_order");
        binds.push(rusqlite::types::Value::Integer(s));
    }

    if !named.is_empty() {
        let assignments = named
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c} = ?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        binds.push(rusqlite::types::Value::Text(id.to_string()));
        let sql = format!("UPDATE nodes SET {assignments} WHERE id = ?{}", binds.len());
        let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
        if rows == 0 {
            return Err(AppError::NotFound(format!("node {id}")));
        }
    }

    match reparent_to {
        // Absent: nothing asked to move.
        None => {}
        Some(new_parent) => reparent(conn, id, new_parent.as_deref())?,
    }

    get(conn, id)
}

/// Move a node under a new parent without changing its position among
/// its siblings.
///
/// Thin wrapper over [`move_many`] so a single-node move and a
/// multi-select drag cannot diverge — the cycle check, the type matrix
/// and the `root_id` cascade all live in one place.
pub fn reparent(
    conn: &mut Connection,
    id: &str,
    new_parent_id: Option<&str>,
) -> Result<(), AppError> {
    let node = get(conn, id)?;
    // Keep the node's current slot: read its index among the old
    // siblings and reuse it against the new set.
    let index = sibling_index(conn, &node, id)?;
    move_many(
        conn,
        std::slice::from_ref(&id.to_string()),
        new_parent_id,
        index,
    )
}

/// Delete a node and its whole subtree.
///
/// The `ON DELETE CASCADE` on `parent_id` does the teardown, and the
/// `root_id` cascade on `projects` covers deleting a project. The only
/// thing this adds is reporting a missing id instead of silently
/// succeeding.
pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM nodes WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("node {id}")));
    }
    Ok(())
}

/// Move one or many nodes to a new sibling position under
/// `new_parent_id`.
///
/// `node_ids` is in the order the nodes should appear at the
/// destination, so a multi-select drag keeps the user's pick order.
/// `None` for the parent is only meaningful for a module, whose parent
/// is the project rather than a node.
///
/// MUST be called inside `db::with_tx` — this issues one UPDATE per
/// sibling to renumber, and the caller wants those to land together.
///
/// The five things that have to be true afterwards, in order:
/// 1. no node sits inside its own subtree (cycle)
/// 2. every moved node's type is allowed under the new parent
/// 3. the new parent's siblings are a contiguous `0..N` run
/// 4. the whole subtree's `root_id` matches the new parent's
/// 5. each moved row carries the new `parent_id` and its new slot
pub fn move_many(
    conn: &mut Connection,
    node_ids: &[String],
    new_parent_id: Option<&str>,
    target_index: i32,
) -> Result<(), AppError> {
    if node_ids.is_empty() {
        return Ok(());
    }

    // Load the movers up front. Everything below needs their types and
    // current owners, and reading once keeps the checks consistent with
    // each other even if a trigger or a concurrent writer is in play.
    let mut movers = Vec::with_capacity(node_ids.len());
    for id in node_ids {
        movers.push(get(conn, id)?);
    }

    // 1. Cycle check. A collection nested under one of its own
    //    descendants would make every tree walk below this point loop.
    let moved_subtrees = subtree_ids(conn, node_ids)?;
    let subtree_set: std::collections::HashSet<&String> = moved_subtrees.iter().collect();
    if let Some(parent) = new_parent_id {
        if subtree_set.contains(&parent.to_string()) {
            return Err(AppError::Validation(format!(
                "cannot move a node under its own descendant ({parent})"
            )));
        }
    }

    // 2. Type matrix. A module has no node parent, so `None` is the only
    //    legal landing spot for one; anything else needs a parent whose
    //    type accepts it.
    let new_root_id = match new_parent_id {
        None => {
            for m in &movers {
                if m.kind != ResourceNodeType::Module {
                    return Err(AppError::Validation(format!(
                        "only a module can sit at the top of a project; \
                         node {} is a {}",
                        m.id,
                        m.kind.as_str()
                    )));
                }
            }
            movers[0].root_id.clone()
        }
        Some(parent) => {
            let parent_node = get(conn, parent)?;
            for m in &movers {
                if !m.kind.may_under(parent_node.kind) {
                    return Err(AppError::Validation(format!(
                        "a {} cannot sit under a {}",
                        m.kind.as_str(),
                        parent_node.kind.as_str()
                    )));
                }
            }
            parent_node.root_id
        }
    };

    // 3. Reorder the new parent's children. Scoped to the destination,
    //    which is why this is one uniform query — the old layout had to
    //    branch on "module or collection parent" because they were
    //    different tables.
    let siblings: Vec<String> = match new_parent_id {
        Some(p) => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![p], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![&new_root_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
    };

    let mut order: Vec<String> = siblings
        .into_iter()
        .filter(|id| !node_ids.contains(id))
        .collect();
    let mut idx = target_index.max(0) as usize;
    if idx > order.len() {
        idx = order.len();
    }
    for (offset, id) in node_ids.iter().enumerate() {
        order.insert(idx + offset, id.clone());
    }

    let mut update = conn.prepare("UPDATE nodes SET sort_order = ?1 WHERE id = ?2")?;
    for (i, id) in order.iter().enumerate() {
        update.execute(params![i as i64, id])?;
    }
    drop(update);

    // 4. Re-derive `root_id` across each moved subtree. Derived value,
    //    so a boundary crossing has to reach every descendant — doing
    //    it only for the named rows leaves the subtree filed under the
    //    old project and invisible to `list_by_root`, which is the
    //    same shape of bug as the collection `module_id` drift that
    //    `reassign_subtree_module` fixes on the old layout.
    if movers.iter().any(|m| m.root_id != new_root_id) {
        let mut update_root = conn.prepare("UPDATE nodes SET root_id = ?1 WHERE id = ?2")?;
        for id in &moved_subtrees {
            update_root.execute(params![&new_root_id, id])?;
        }
    }

    // 5. Attach the movers.
    let mut attach = conn.prepare("UPDATE nodes SET parent_id = ?1 WHERE id = ?2")?;
    for id in node_ids {
        attach.execute(params![new_parent_id, id])?;
    }
    Ok(())
}

/// Transfer a module to a different project, at a given top-level slot.
///
/// The one move [`move_many`] cannot express. With `new_parent_id` of
/// `None` it has no parent row to read a root from, so it keeps the
/// module's current `root_id` — correct for "reorder my modules", wrong
/// for "move this module to another project", which is what the MCP
/// `module_move` tool has always meant.
///
/// Separate rather than an extra argument on `move_many` because the two
/// differ in more than a parameter value: a boundary crossing has to
/// renumber the *old* project's top-level siblings too, or that project
/// is left with a gap in its `sort_order` run.
///
/// `target_index` of `None` appends. MUST be called inside `db::with_tx`
/// — both projects' orderings have to land together.
pub fn move_to_root(
    conn: &mut Connection,
    id: &str,
    new_root_id: &str,
    target_index: Option<i32>,
) -> Result<ResourceNode, AppError> {
    let node = get(conn, id)?;
    if node.kind != ResourceNodeType::Module {
        return Err(AppError::Validation(format!(
            "only a module can move between projects; node {id} is a {}",
            node.kind.as_str()
        )));
    }
    // A typo'd project id would otherwise file the subtree under a root
    // no project row backs, where nothing can ever list it again.
    let dest_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![new_root_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !dest_exists {
        return Err(AppError::NotFound(format!("project {new_root_id}")));
    }
    if node.root_id == new_root_id && target_index.is_none() {
        return Ok(node);
    }

    let moved_subtrees = subtree_ids(conn, &[id.to_string()])?;

    // Close the gap in the project being left, unless that is also the
    // destination — in which case the insert below renumbers anyway and
    // running the compaction first would only churn the same rows.
    //
    // The departing module is excluded: at this point it is still filed
    // under the old root, so counting it would leave its old slot
    // occupied and shift nothing up.
    if node.root_id != new_root_id {
        renumber_top_level(conn, &node.root_id, Some(id))?;
    }

    // Slot the module into the destination, pulling the rest of that
    // project's top level up around it.
    let mut order = top_level_ids(conn, new_root_id)?
        .into_iter()
        .filter(|existing| existing != id)
        .collect::<Vec<_>>();
    let idx = match target_index {
        Some(i) => (i.max(0) as usize).min(order.len()),
        None => order.len(),
    };
    order.insert(idx, id.to_string());
    write_order(conn, &order)?;

    // Re-derive `root_id` across the subtree, for the same reason
    // `move_many` does: the named row alone would leave every descendant
    // filed under the old project and invisible to `list_by_root`.
    let mut update_root = conn.prepare("UPDATE nodes SET root_id = ?1 WHERE id = ?2")?;
    for sid in &moved_subtrees {
        update_root.execute(params![new_root_id, sid])?;
    }
    drop(update_root);

    get(conn, id)
}

/// Direct children of one node, in display order.
///
/// `kind` narrows to a single type. With the unified tree this is the
/// only query a "list the endpoints in this collection" needs — the old
/// `endpoints::list_by_collection` existed because an endpoint's parent
/// could only ever be a collection, which is no longer true.
///
/// Not recursive: for a whole subtree use [`subtree_ids`] with
/// [`list_by_root_and_type`], or walk parents from the root.
pub fn list_children(
    conn: &mut Connection,
    parent_id: &str,
    kind: Option<ResourceNodeType>,
) -> Result<Vec<ResourceNode>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at \
         FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![parent_id], row_to_node)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    Ok(match kind {
        Some(k) => rows.into_iter().filter(|n| n.kind == k).collect(),
        None => rows,
    })
}

/// Ids of a project's top-level nodes, in display order.
fn top_level_ids(conn: &mut Connection, root_id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let ids = stmt
        .query_map(params![root_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Write `0..N` into `sort_order` for the given ids, in the given order.
fn write_order(conn: &mut Connection, ids: &[String]) -> Result<(), AppError> {
    let mut update = conn.prepare("UPDATE nodes SET sort_order = ?1 WHERE id = ?2")?;
    for (i, id) in ids.iter().enumerate() {
        update.execute(params![i as i64, id])?;
    }
    Ok(())
}

/// Collapse a project's top-level ordering back to a contiguous `0..N`
/// run after a member left it. `exclude` is the member that left — it is
/// still filed under this root at the moment this runs, so leaving it in
/// would keep its vacated slot occupied.
fn renumber_top_level(
    conn: &mut Connection,
    root_id: &str,
    exclude: Option<&str>,
) -> Result<(), AppError> {
    let ids = top_level_ids(conn, root_id)?
        .into_iter()
        .filter(|id| Some(id.as_str()) != exclude)
        .collect::<Vec<_>>();
    write_order(conn, &ids)
}

// -------------------------------------------------------------------------
// Internals.
// -------------------------------------------------------------------------

/// Check that a placement is legal before writing it.
///
/// Four things can be wrong, and all four used to be caught by foreign
/// keys that no longer exist:
/// * a node under a parent whose type does not accept it
/// * a module given a node parent
/// * a `root_id` that disagrees with the parent's, which would file a
///   node under one project while rendering it in another's tree
/// * a top-level node that names no project at all, or one that does not
///   exist
///
/// The last one is the one this function used to let through: both
/// transports default an absent `rootId` to the empty string, and a
/// top-level node has no `parentId` to derive it from, so the empty
/// string reached `INSERT` and SQLite reported
/// `FOREIGN KEY constraint failed`. That surfaced as a 500 with
/// `code: "sqlite"` — a transport failure for what is really a caller
/// mistake, and one the error envelope tells a client it cannot
/// distinguish from a genuine server fault.
fn validate_placement(
    conn: &mut Connection,
    kind: ResourceNodeType,
    parent_id: Option<&str>,
    root_id: &str,
) -> Result<(), AppError> {
    let Some(parent_id) = parent_id else {
        if kind != ResourceNodeType::Module {
            return Err(AppError::Validation(format!(
                "a {} needs a parent node; only a module can sit at the top of a project",
                kind.as_str()
            )));
        }
        // A module's parent is the project itself, which the row cannot
        // reference — so the project has to be named, one way or the
        // other.
        if root_id.is_empty() {
            return Err(AppError::Validation(
                "a top-level node needs its project: pass rootId, or a parentId to derive it from"
                    .into(),
            ));
        }
        if crate::db::projects::get(conn, root_id).is_err() {
            return Err(AppError::Validation(format!(
                "no project with id {root_id:?}; a top-level node must name the project it belongs to"
            )));
        }
        return Ok(());
    };

    let parent = get(conn, parent_id)?;
    if !kind.may_under(parent.kind) {
        return Err(AppError::Validation(format!(
            "a {} cannot sit under a {}",
            kind.as_str(),
            parent.kind.as_str()
        )));
    }
    if parent.root_id != root_id {
        return Err(AppError::Validation(format!(
            "node belongs to project {} but its parent belongs to {}",
            root_id, parent.root_id
        )));
    }
    Ok(())
}

/// An endpoint needs exactly one detail row; a module or collection
/// must not have one.
///
/// Checked up front so a caller who forgets — or passes a detail for a
/// collection — gets a validation error naming the mistake, instead of
/// a tree where opening the item reports a missing row.
fn check_detail_agreement(
    node: &ResourceNode,
    detail: Option<&EndpointDetail>,
) -> Result<(), AppError> {
    match (node.kind, detail) {
        // Order matters: the valid endpoint case has to be matched
        // before the "no detail allowed" arm below, which would
        // otherwise swallow it.
        (ResourceNodeType::Endpoint, Some(d)) if d.node_id == node.id => Ok(()),
        (ResourceNodeType::Endpoint, Some(EndpointDetail { node_id, .. })) => {
            Err(AppError::Validation(format!(
                "endpoint detail is for node {node_id} but the node is {}",
                node.id
            )))
        }
        (ResourceNodeType::Endpoint, None) => Err(AppError::Validation(format!(
            "endpoint node {} needs an endpoint detail payload",
            node.id
        ))),
        (_, Some(_)) => Err(AppError::Validation(format!(
            "a {} node takes no endpoint detail payload",
            node.kind.as_str()
        ))),
        (_, None) => Ok(()),
    }
}

/// Zero-based position of `id` among the siblings it currently has.
fn sibling_index(conn: &mut Connection, node: &ResourceNode, id: &str) -> Result<i32, AppError> {
    let ids: Vec<String> = match &node.parent_id {
        Some(p) => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![p], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![&node.root_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
    };
    Ok(ids.iter().position(|x| x == id).unwrap_or(0) as i32)
}

fn row_to_node(row: &Row<'_>) -> Result<ResourceNode, rusqlite::Error> {
    let type_str: String = row.get(3)?;
    let kind = ResourceNodeType::parse(&type_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(super::JsonColError(format!(
                "type: unrecognised node type {type_str:?}"
            ))),
        )
    })?;
    Ok(ResourceNode {
        id: row.get(0)?,
        root_id: row.get(1)?,
        parent_id: row.get(2)?,
        kind,
        name: row.get(4)?,
        description: row.get(5)?,
        sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "nodes_test.rs"]
mod tests;
