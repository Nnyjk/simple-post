//! CRUD for the `environments` table.
//!
//! Each environment holds a per-(service × module) URL map
//! (`moduleBaseUrls`) and a free-form variable pool. That map is the only
//! URL layer: a cell is either set or it is not, and a missing cell
//! resolves to `''` rather than falling back to another module's value or
//! to the retired environment-level map. The `base_urls_json` column still
//! exists — dropping a column means rebuilding the table, which means a
//! `SCHEMA_VERSION` bump, which means `ensure_current_schema` deletes the
//! user's store — but nothing here reads or writes it; the one reader is
//! the migration in `db::migrate`.
//!
//! `set_active` is a transactional
//! "demote others, promote this" — the transaction must hold the
//! write lock across both steps or two simultaneous set_active calls
//! could leave two envs active. We don't take the lock here — the
//! caller is expected to wrap `set_active` in `db::with_tx`.

use std::collections::HashMap;

use rusqlite::{params, Connection, Row};

use crate::domain::{Environment, EnvironmentPatch};
use crate::error::AppError;

/// Column list every read here shares, in [`row_to_env`]'s order.
///
/// One string rather than four copies: the two URL columns sit next to
/// each other, and a read that named only one would silently produce an
/// environment whose modules all resolve to `''`.
const COLUMNS: &str =
    "id, project_id, name, is_active, base_urls_json, module_base_urls_json, variables_json";

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// All environments under a project, ordered by `name` for stable
/// display in the picker.
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<Environment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE project_id = ?1 \
         ORDER BY name ASC, id ASC"
    ))?;
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         ORDER BY project_id ASC, name ASC, id ASC"
    ))?;
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE project_id = ?1 AND is_active = 1 \
         LIMIT 1"
    ))?;
    let mut rows = stmt.query(params![project_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_env(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

pub fn get(conn: &mut Connection, id: &str) -> Result<Environment, AppError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM environments WHERE id = ?1"))?;
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
/// The `env_*` mutating tools (update / delete / set_var / set_active)
/// used to call `check_permission(.., "global", ..)`
/// with the literal `"global"` as a stand-in for "any project". That
/// was a permission-bypass waiting to happen: any project whose agent
/// permission row was absent defaulted to `Permission::ReadCall`
/// (see `db::permissions::get`), so the "global" lookup would silently
/// succeed for `env_update` / `env_set_var` and
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
    let module_base_urls_json = serde_json::to_string(&e.module_base_urls)?;
    let variables_json = serde_json::to_string(&e.variables)?;
    // `base_urls_json` is deliberately absent from this list. Naming it
    // would keep writing a column nothing reads, and carrying a caller's
    // legacy `baseUrls` into it would leave a blob that looks live to the
    // next reader. Leaving it out lets the schema default apply, which is
    // the empty value it holds on every row this build writes.
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, is_active, module_base_urls_json, variables_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            e.id,
            e.project_id,
            e.name,
            e.is_active as i64,
            module_base_urls_json,
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
    if let Some(m) = &patch.module_base_urls {
        sets.push(format!("module_base_urls_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(m)?));
    }
    // `patch.base_urls` is accepted and **ignored** — the column behind it
    // is dead (see the module docs). Writing it would make a retired
    // layer look current to the next reader, and dropping the field from
    // the patch type would fail a payload from an older client outright,
    // so silently skipping it is the trade.
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

/// Set or clear one `moduleBaseUrls[def_id][module_id]` cell.
///
/// `url = None` removes the cell, and that is a real answer rather than a
/// hole: (service × module) is the only layer there is, so a cleared cell
/// means this module has no address for this service and resolution stops
/// there. It does not fall back to another module's value or to the
/// retired environment-level map — two modules pointing one service at
/// different hosts is the normal case, not an error to be smoothed over.
///
/// The endpoint's module is not verified against the environment's
/// project here: the pair only becomes a URL when an endpoint actually
/// resolves it, and `base_url_definitions.project_id_of` is the check that
/// keeps a cross-project service from being named at all.
///
/// MUST be called inside `db::with_tx`.
///
/// Returns the post-update [`Environment`] for the renderer to echo.
pub fn set_module_base_url(
    conn: &mut Connection,
    env_id: &str,
    def_id: &str,
    module_id: &str,
    url: Option<&str>,
) -> Result<Environment, AppError> {
    // Same load → mutate → write pattern as `set_var`, for the same
    // reason: one atomic UPDATE of the JSON column keeps a reader from
    // seeing a half-written map. The nested map is only created when a
    // value is actually being set, so clearing the last cell in a service
    // does not leave an empty `{}` behind it.
    let mut env = get(conn, env_id)?;
    match url {
        Some(u) => {
            env.module_base_urls
                .entry(def_id.to_string())
                .or_default()
                .insert(module_id.to_string(), u.to_string());
        }
        None => {
            if let Some(per_module) = env.module_base_urls.get_mut(def_id) {
                per_module.remove(module_id);
                if per_module.is_empty() {
                    env.module_base_urls.remove(def_id);
                }
            }
        }
    }
    let module_base_urls_json = serde_json::to_string(&env.module_base_urls)?;
    let rows = conn.execute(
        "UPDATE environments SET module_base_urls_json = ?1 WHERE id = ?2",
        params![module_base_urls_json, env_id],
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
    let module_base_urls_json: String = row.get(5)?;
    let variables_json: String = row.get(6)?;
    let parse_err = |e: serde_json::Error, col: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(JsonColError(format!("{col}: {e}"))),
        )
    };
    // `base_urls` is decoded but never used: the column is kept only so it
    // survives, and decoding it is what keeps the wire field honest for a
    // client that still sends one. A malformed blob therefore still fails
    // the read rather than being silently reported as an empty map.
    let base_urls: HashMap<String, String> =
        serde_json::from_str(&base_urls_json).map_err(|e| parse_err(e, "base_urls_json"))?;
    let module_base_urls: HashMap<String, HashMap<String, String>> =
        serde_json::from_str(&module_base_urls_json)
            .map_err(|e| parse_err(e, "module_base_urls_json"))?;
    let variables: HashMap<String, String> =
        serde_json::from_str(&variables_json).map_err(|e| parse_err(e, "variables_json"))?;
    Ok(Environment {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        is_active: row.get::<_, i64>(3)? != 0,
        base_urls,
        module_base_urls,
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
