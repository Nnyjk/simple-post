// ===========================================================================
// region: db status
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::error::AppError;

/// Unix-ms timestamp of the last destructive schema rebuild, or `None`
/// when this workspace has never been reset.
///
/// Written by [`crate::db::init`] only when it had to replace a
/// `store.db` written by an older layout — there is no migration path,
/// so the previous file is dropped rather than upgraded. The renderer
/// reads this once during bootstrap and announces the reset.
///
/// Without it the user opens the app to an empty tree with no
/// explanation, which is indistinguishable from never having had any
/// projects — and they have no way to tell data loss from an empty
/// workspace.
#[tauri::command]
pub fn db_recreated_at(state: State<DbPool>) -> Result<Option<i64>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::preferences::get(c, crate::db::preferences::keys::DB_RECREATED_AT)
    })
}
