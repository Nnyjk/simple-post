// ===========================================================================
// region: environments
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::CreateEnvironmentOpts;

#[tauri::command]
pub fn environments_list(
    state: State<DbPool>,
    project_id: Option<String>,
) -> Result<Vec<Environment>, AppError> {
    crate::db::with_conn(&state, |c| match project_id.as_deref() {
        Some(project_id) => crate::db::environments::list_by_project(c, project_id),
        None => crate::db::environments::list_all(c),
    })
}

#[tauri::command]
pub fn environments_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateEnvironmentOpts>,
) -> Result<Environment, AppError> {
    let opts = opts.unwrap_or_default();
    let env = Environment {
        id: uid(),
        project_id,
        name,
        base_urls: opts.base_urls.unwrap_or_default(),
        variables: opts.variables.unwrap_or_default(),
        is_active: opts.make_active.unwrap_or(false),
    };
    crate::db::with_tx(&state, |c| {
        crate::db::environments::create(c, &env)?;
        Ok(env.clone())
    })
}

#[tauri::command]
pub fn environments_update(
    state: State<DbPool>,
    id: String,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn environments_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::delete(c, &id))
}

#[tauri::command]
pub fn environments_set_var(
    state: State<DbPool>,
    env_id: String,
    key: String,
    value: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_var(c, &env_id, &key, value.as_deref())
    })
}

/// Rename an environment variable in one transaction.
///
/// The renderer used to model a rename as `set_var(old, None)` followed
/// by `set_var(new, value)` — two commands, two transactions, with a
/// window in between where the variable does not exist. Anything
/// interrupting that window (crash, or the second command failing)
/// destroyed the variable. One call, one `BEGIN IMMEDIATE`, no window.
#[tauri::command]
pub fn environments_rename_var(
    state: State<DbPool>,
    env_id: String,
    from_key: String,
    to_key: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::rename_var(c, &env_id, &from_key, &to_key)
    })
}

#[tauri::command]
pub fn environments_set_baseurl(
    state: State<DbPool>,
    env_id: String,
    def_id: String,
    url: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_base_url(c, &env_id, &def_id, url.as_deref())
    })
}

#[tauri::command]
pub fn environments_set_active(
    state: State<DbPool>,
    env_id: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::set_active(c, &env_id))
}
