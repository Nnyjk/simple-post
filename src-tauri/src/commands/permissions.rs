// ===========================================================================
// region: permissions
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::error::AppError;
use crate::permissions::Permission;

#[tauri::command]
pub fn permissions_get(state: State<DbPool>, project_id: String) -> Result<Permission, AppError> {
    crate::db::with_conn(&state, |c| crate::db::permissions::get(c, &project_id))
}

#[tauri::command]
pub fn permissions_set(
    state: State<DbPool>,
    project_id: String,
    permission: Permission,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::permissions::set(c, &project_id, permission)
    })
}
