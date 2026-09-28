//! CRUD for the `environments` table.
//!
//! Each environment holds a per-definition URL map (`baseUrls`) and a
//! free-form variable pool. `set_active` is a transactional
//! "demote others, promote this" — the transaction must hold the
//! write lock across both steps or two simultaneous set_active calls
//! could leave two envs active. We don't take the lock here — the
//! caller is expected to wrap `set_active` in `db::with_tx`.

use std::collections::HashMap;

use rusqlite::{params, Connection, Row};

use crate::domain::{Environment, EnvironmentPatch};
use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All environments under a project, ordered by `name` for stable
/// display in the picker.
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<Environment>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, is_active, base_urls_json, variables_json \
         FROM environments \
         WHERE project_id = ?1 \
         ORDER BY name ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_env)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// All environments across every project — used by the REST
/// `GET /api/environments` when no `projectId` filter is supplied.
pub fn list_all(conn: &mut Connection) -> Result<Vec<Environment>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, is_active, base_urls_json, variables_json \
         FROM environments \
         ORDER BY project_id ASC, name ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![], row_to_env)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// The single active env for the project, or `None` if none is
/// active. The schema doesn't enforce a single-active-row invariant
/// (the UI does), so this returns whatever row has `is_active = 1`.
/// `set_active` keeps that invariant intact.
pub fn get_active(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Option<Environment>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, is_active, base_urls_json, variables_json \
         FROM environments \
         WHERE project_id = ?1 AND is_active = 1 \
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![project_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_env(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

pub fn get(conn: &mut Connection, id: &str) -> Result<Environment, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, is_active, base_urls_json, variables_json \
         FROM environments WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_env(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("environment {id}"))),
    }
}

/// Look up the `project_id` owning `env_id`. Returns `NotFound` when
/// the env id is unknown — the caller (a mutating MCP tool) needs that
/// resolution to apply the per-project permission check; treating an
/// unknown id as a silent "global" would be a permission bypass.
///
/// The five `env_*` mutating tools (update / delete / set_var / set_baseurl
/// / set_active) used to call `check_permission(.., "global", ..)`
/// with the literal `"global"` as a stand-in for "any project". That
/// was a permission-bypass waiting to happen: any project whose agent
/// permission row was absent defaulted to `Permission::ReadCall`
/// (see `db::permissions::get`), so the "global" lookup would silently
/// succeed for `env_update` / `env_set_var` / `env_set_baseurl` and
/// `env_set_active` (which check `ReadCall`), and for `env_delete`
/// (which checked `Full` and so still got denied — but only by
/// accident). This helper is the missing piece that closes that gap:
///
///   tool_env_update(env_id="e_unknown") ⇒ lookup_project_id fails
///   ⇒ NotFound ⇒ handler returns NotFound before touching anything.
pub fn lookup_project_id(conn: &mut Connection, env_id: &str) -> Result<String, AppError> {
    let pid: Option<String> = conn
        .query_row(
            "SELECT project_id FROM environments WHERE id = ?1",
            params![env_id],
            |r| r.get(0),
        )
        .ok();
    pid.ok_or_else(|| AppError::NotFound(format!("environment {env_id}")))
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

pub fn create(conn: &mut Connection, e: &Environment) -> Result<(), AppError> {
    let base_urls_json = serde_json::to_string(&e.base_urls)?;
    let variables_json = serde_json::to_string(&e.variables)?;
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, is_active, base_urls_json, variables_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            e.id,
            e.project_id,
            e.name,
            e.is_active as i64,
            base_urls_json,
            variables_json,
        ],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
    // Note: `is_active` is intentionally *not* patched here — use
    // `set_active` to flip it. That keeps the "exactly one active"
    // invariant concentrated in one function and avoids surprising
    // a caller who patches `name` and accidentally breaks the
    // single-active invariant.
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(name.clone()));
    }
    if let Some(b) = &patch.base_urls {
        sets.push(format!("base_urls_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(b)?));
    }
    if let Some(v) = &patch.variables {
        sets.push(format!("variables_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(v)?));
    }
    if let Some(a) = patch.is_active {
        sets.push(format!("is_active = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Integer(a as i64));
    }

    if sets.is_empty() {
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE environments SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    get(conn, id)
}

/// Promote `id` to active and demote all other envs of the same
/// project. MUST be called inside `db::with_tx` — the two
/// `UPDATE`s have to take the write lock together or a concurrent
/// `set_active` could leave two envs active.
///
/// Returns the post-update environment so the caller can echo it
/// back to the renderer.
pub fn set_active(conn: &mut Connection, id: &str) -> Result<Environment, AppError> {
    // Look up the project so we can scope the demotion. This must
    // happen before the updates so we can short-circuit on a missing id.
    let project_id: Option<String> = conn
        .query_row(
            "SELECT project_id FROM environments WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    let project_id = match project_id {
        Some(p) => p,
        None => return Err(AppError::NotFound(format!("environment {id}"))),
    };
    conn.execute(
        "UPDATE environments SET is_active = 0 \
         WHERE project_id = ?1 AND id != ?2",
        params![project_id, id],
    )?;
    conn.execute(
        "UPDATE environments SET is_active = 1 WHERE id = ?1",
        params![id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM environments WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    Ok(())
}

/// Set or clear a single variable on an environment. `value = None`
/// removes the key entirely (so `set_var(env, "k", None)` is the
/// delete-by-key path). `value = Some(v)` upserts `k → v`.
///
/// MUST be called inside `db::with_tx`.
///
/// Returns the post-update [`Environment`] so the caller can echo it
/// back to the renderer without a second read.
pub fn set_var(
    conn: &mut Connection,
    env_id: &str,
    key: &str,
    value: Option<&str>,
) -> Result<Environment, AppError> {
    // Read once, mutate the in-memory map, write the JSON column.
    // Cheaper than a JSON-extend SQL function for a single key, and
    // keeps the round-trip shape identical to `get`.
    let mut env = get(conn, env_id)?;
    match value {
        Some(v) => {
            env.variables.insert(key.to_string(), v.to_string());
        }
        None => {
            env.variables.remove(key);
        }
    }
    let variables_json = serde_json::to_string(&env.variables)?;
    let rows = conn.execute(
        "UPDATE environments SET variables_json = ?1 WHERE id = ?2",
        params![variables_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

/// Rename a single environment variable in one shot: `from_key` becomes
/// `to_key`, keeping its value.
///
/// The renderer used to model a rename as `set_var(from_key, None)` followed
/// by `set_var(to_key, value)` — two transactions with a window in between
/// where the variable does not exist. Anything interrupting that window
/// (crash, or the second call failing) destroyed the variable. This is one
/// `UPDATE` of the JSON column, so the key is never observably missing.
///
/// MUST be called inside `db::with_tx`.
///
/// Errors:
///   - [`AppError::NotFound`] if the environment doesn't exist
///   - [`AppError::Validation`] if `from_key` is absent (nothing to rename),
///     if `to_key` already exists (refusing to clobber another variable), or
///     if `from_key == to_key` (a no-op rename is almost always a UI bug)
///
/// Returns the post-update [`Environment`] so the caller echoes the
/// authoritative row rather than an optimistic guess.
pub fn rename_var(
    conn: &mut Connection,
    env_id: &str,
    from_key: &str,
    to_key: &str,
) -> Result<Environment, AppError> {
    if from_key == to_key {
        return Err(AppError::Validation(format!(
            "rename_var: from_key and to_key are both {from_key:?}"
        )));
    }
    let mut env = get(conn, env_id)?;
    let value = env.variables.remove(from_key).ok_or_else(|| {
        AppError::Validation(format!(
            "rename_var: environment {env_id} has no variable {from_key:?}"
        ))
    })?;
    if env.variables.contains_key(to_key) {
        // Restore the key we just removed so an abort leaves the map untouched.
        env.variables.insert(from_key.to_string(), value);
        return Err(AppError::Validation(format!(
            "rename_var: environment {env_id} already has a variable {to_key:?}"
        )));
    }
    env.variables.insert(to_key.to_string(), value);

    // Same single-UPDATE shape as `set_var`: the `variables_json` column is
    // written whole, which is atomic, so the rename has no intermediate state.
    let variables_json = serde_json::to_string(&env.variables)?;
    let rows = conn.execute(
        "UPDATE environments SET variables_json = ?1 WHERE id = ?2",
        params![variables_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

/// Set or clear a single `baseUrls[def_id]` slot on an environment.
/// `url = None` removes the slot (so a `def` without a configured URL
/// on this env resolves to ""). `url = Some(u)` upserts `def_id → u`.
///
/// MUST be called inside `db::with_tx`.
///
/// Returns the post-update [`Environment`] for the renderer to echo.
pub fn set_base_url(
    conn: &mut Connection,
    env_id: &str,
    def_id: &str,
    url: Option<&str>,
) -> Result<Environment, AppError> {
    // Same load → mutate → write pattern as `set_var`. The two
    // helpers stay split so the SQL stays trivial to grep and the
    // JSON column names (`variables_json` / `base_urls_json`) are
    // visible at the call site.
    let mut env = get(conn, env_id)?;
    match url {
        Some(u) => {
            env.base_urls.insert(def_id.to_string(), u.to_string());
        }
        None => {
            env.base_urls.remove(def_id);
        }
    }
    let base_urls_json = serde_json::to_string(&env.base_urls)?;
    let rows = conn.execute(
        "UPDATE environments SET base_urls_json = ?1 WHERE id = ?2",
        params![base_urls_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

// -------------------------------------------------------------------------
// Row mapper.
// -------------------------------------------------------------------------

fn row_to_env(row: &Row<'_>) -> Result<Environment, rusqlite::Error> {
    let base_urls_json: String = row.get(4)?;
    let variables_json: String = row.get(5)?;
    let parse_err = |e: serde_json::Error, col: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(JsonColError(format!("{col}: {e}"))),
        )
    };
    let base_urls: HashMap<String, String> =
        serde_json::from_str(&base_urls_json).map_err(|e| parse_err(e, "base_urls_json"))?;
    let variables: HashMap<String, String> =
        serde_json::from_str(&variables_json).map_err(|e| parse_err(e, "variables_json"))?;
    Ok(Environment {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        is_active: row.get::<_, i64>(3)? != 0,
        base_urls,
        variables,
    })
}

/// Tiny `StdError` wrapper around a string. Needed because rusqlite
/// demands `Box<dyn StdError + Send + Sync + 'static>` for the
/// `FromSqlConversionFailure` payload.
#[derive(Debug)]
struct JsonColError(String);

impl std::fmt::Display for JsonColError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonColError {}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "environments_test.rs"]
mod tests;
