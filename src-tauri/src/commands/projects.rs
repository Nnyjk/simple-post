// ===========================================================================
// region: projects
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::CreateProjectOpts;

#[tauri::command]
pub fn projects_list(state: State<DbPool>) -> Result<Vec<Project>, AppError> {
    crate::db::with_conn(&state, crate::db::projects::list)
}

#[tauri::command]
pub fn projects_get(state: State<DbPool>, id: String) -> Result<Project, AppError> {
    crate::db::with_conn(&state, |c| crate::db::projects::get(c, &id))
}

#[tauri::command]
pub fn projects_create(
    state: State<DbPool>,
    name: String,
    opts: Option<CreateProjectOpts>,
) -> Result<Project, AppError> {
    let opts = opts.unwrap_or_default();
    let now = crate::domain::now_ms();
    let project = Project {
        id: uid(),
        name,
        description: opts.description,
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: opts
            .color
            .unwrap_or_else(crate::domain::default_project_color),
        created_at: now,
        updated_at: now,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::projects::create(c, &project)?;
        Ok(project.clone())
    })
}

#[tauri::command]
pub fn projects_update(
    state: State<DbPool>,
    id: String,
    patch: ProjectPatch,
) -> Result<Project, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::projects::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn projects_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::projects::delete(c, &id))
}
