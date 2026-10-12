// ===========================================================================
// region: base_url_defs
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::{BaseUrlDefPatch, CreateBaseUrlDefOpts};

/// Every service of the project, flat.
///
/// 「服务应该绑定到项目，这样每一个模块都会全部包含这些服务」 — a
/// module owns no services, so there is one list per project and every
/// module of it can resolve every row. The list is flat because the
/// endpoint's `baseUrlDefinitionId` is flat too, and because an
/// endpoint's module picks a *value* out of
/// `env.moduleBaseUrls[defId][moduleId]` rather than a subset of the
/// services.
#[tauri::command]
pub fn base_url_defs_list(
    state: State<DbPool>,
    project_id: String,
) -> Result<Vec<BaseUrlDefinition>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::base_url_defs::list_by_project(c, &project_id)
    })
}

/// Add a service to the **project's** list.
///
/// The project is the scope. It stays `None` afterwards if it had no ★:
/// creating a service is not the same as choosing one, and a project
/// whose endpoints have no default go out with no base URL rather than
/// one the user never picked.
#[tauri::command]
pub fn base_url_defs_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateBaseUrlDefOpts>,
) -> Result<BaseUrlDefinition, AppError> {
    let opts = opts.unwrap_or_default();
    let def = BaseUrlDefinition {
        id: uid(),
        name,
        description: opts.description,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::create(c, &def, &project_id)?;
        // Returns the stored row rather than the input: the renderer
        // swaps it into its list, and `get` is the only read that agrees
        // with what the database now holds.
        crate::db::base_url_defs::get(c, &def.id)
    })
}

#[tauri::command]
pub fn base_url_defs_update(
    state: State<DbPool>,
    id: String,
    patch: BaseUrlDefPatch,
) -> Result<BaseUrlDefinition, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::update(c, &id, patch.name.clone(), patch.description.clone())
    })
}

/// Delete a service. The project's ★ pointer is cleared with it when it
/// named this row — there is nothing to promote, since a project has
/// one ★ and no rule for choosing its replacement.
#[tauri::command]
pub fn base_url_defs_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::base_url_defs::delete(c, &id))
}

/// Point the project's ★ at one of its own services, or take the ★ away
/// (`def_id = None`).
///
/// Returns the `Project` rather than nothing: `defaultBaseUrlDefinitionId`
/// is live again — it is what an endpoint naming no service resolves
/// against — so the caller would otherwise have to guess the new value
/// from a void answer and re-read to confirm the write landed.
#[tauri::command]
pub fn base_url_defs_set_default(
    state: State<DbPool>,
    project_id: String,
    def_id: Option<String>,
) -> Result<Project, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::set_default(c, &project_id, def_id.as_deref())?;
        crate::db::projects::get(c, &project_id)
    })
}
