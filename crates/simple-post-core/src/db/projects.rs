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
/// `Option<Option<T>>` (`description`,
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
    // `patch.base_url` is accepted and **ignored**. The `projects` table
    // has never had a `base_url` column — a project's URL lives in an
    // environment's `moduleBaseUrls`, under a module's service — so
    // emitting a `SET base_url = ?` for it turned `PATCH {"baseUrl": "…"}`
    // into a `no such column` error, i.e. a caller mistake surfaced as a
    // database fault. Keeping the field in the patch type keeps an old
    // client's payload deserialising; silently dropping it is the price,
    // and the comment on the field in `domain.rs` says so.
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

/// The environment every new project is born with.
///
/// A project with zero environments is a dead end rather than a blank
/// slate: `{{var}}` has nothing to resolve against, every base URL
/// resolves to `''`, and the settings panel opens on an empty-state hint
/// telling the user to create one. Seeding the first row means the very
/// next thing they do — add a request — can already use it. It is
/// renameable inline, so the name is a starting point, not a decision.
pub const DEFAULT_ENVIRONMENT_NAME: &str = "默认环境";

/// The module every new project is born with.
///
/// This one *was* deliberately not seeded: a 「默认模块」 the user did not
/// ask for read as a row they had to notice and delete. The pendulum went
/// the other way once the module became the row axis of the base-URL grid
/// **and** the thing the workspace's 服务 block is about. With none, a fresh
/// project showed three separate dead ends at once — the tree's
/// 「该项目还没有模块」, the 前置 URL grid's 「没有模块可归属」, and an
/// empty 服务 block — for a shape the user is going to need anyway. Seeding
/// it turns all three into one thing to fill in. Renameable inline, same
/// contract as the environment.
pub const DEFAULT_MODULE_NAME: &str = "默认模块";

/// Create a project **and** the two rows that make it usable.
///
/// [`create`] alone takes the project row and nothing else, so the project
/// it writes has no environment and no module — the state the create
/// dialog's caller is handed back.
///
/// What is seeded, and just as importantly what is not:
///
/// - Always [`DEFAULT_ENVIRONMENT_NAME`], active, so `{{var}}` and every
///   base URL have somewhere to resolve against the moment the project
///   exists.
/// - Always [`DEFAULT_MODULE_NAME`], so the tree, the base-URL grid and the
///   module workspace all have a row to point at.
/// - **No service, no URL.** The service list belongs to the project but a
///   URL belongs to (environment × module × service), so the URL needs a
///   service name the user has not chosen yet. Seeding a placeholder
///   「默认服务」 and a URL slot for it would mean inventing a name they then
///   have to find and delete, and asking for the URL in the create form
///   would be asking for a value with nowhere to live. The user fills the
///   cartesian grid in settings, where the row they are filling is visible.
///
/// The returned `Project` is the post-write read, so a caller can hand it
/// straight back to the renderer instead of re-fetching.
///
/// The caller must already hold a transaction (`db::with_tx`): the
/// project, its environment and its module have to land together or none
/// should.
pub fn create_with_defaults(conn: &mut Connection, project: &Project) -> Result<Project, AppError> {
    let mut project = project.clone();
    // The caller's struct may carry optimistic state for the form (and,
    // for a re-submitted form, an optimistic ★). Nothing is seeded, so
    // the honest answer is an empty service list and no default — echoing
    // the caller's values back would make the response disagree with what
    // was written.
    project.base_url_definitions = Vec::new();
    project.default_base_url_definition_id = None;

    create(conn, &project)?;

    let env = crate::domain::Environment {
        id: crate::domain::uid(),
        project_id: project.id.clone(),
        name: DEFAULT_ENVIRONMENT_NAME.to_string(),
        base_urls: Default::default(),
        module_base_urls: Default::default(),
        variables: Default::default(),
        is_active: true,
    };
    super::environments::create(conn, &env)?;

    let module = crate::domain::ResourceNode {
        id: crate::domain::uid(),
        root_id: project.id.clone(),
        // A module's parent is the project itself. `nodes` carries no
        // project row, so the parent of a top-level module is stored as
        // `NULL` and `root_id` is what says which project owns it.
        parent_id: None,
        kind: crate::domain::ResourceNodeType::Module,
        name: DEFAULT_MODULE_NAME.to_string(),
        description: None,
        // First and only row, so the order is arbitrary — 0 is what the
        // create-a-module path writes for a first module.
        sort_order: 0,
        created_at: crate::domain::now_ms(),
        updated_at: crate::domain::now_ms(),
    };
    super::nodes::create(conn, &module, None)?;

    Ok(project)
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
