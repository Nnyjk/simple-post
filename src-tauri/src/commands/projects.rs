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

/// 建项目，并种下它的默认环境。
///
/// 命令这一侧只负责线格式：`name` 必填由 IPC 层保证，`opts` 缺席即全空。
#[tauri::command]
pub fn projects_create(
    state: State<DbPool>,
    name: String,
    opts: Option<CreateProjectOpts>,
) -> Result<Project, AppError> {
    crate::project_api::create_project(&state, None, name, opts.unwrap_or_default())
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
