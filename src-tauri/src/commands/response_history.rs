// ===========================================================================
// region: response_history
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::{ResponseHistoryEntry, StoredResponse};
use crate::error::AppError;

/// Every stored response under one project, grouped per endpoint and
/// newest-first inside each group.
///
/// Project-scoped on purpose, the same reason `nodes_list` is: the
/// renderer holds one `Record<endpointId, …>` that a project switch
/// replaces wholesale, so the read has to answer "the active project's
/// whole history" in a single round-trip rather than one call per
/// endpoint.
///
/// `root_id` is required rather than optional — and `db::response_history`
/// offers no unfiltered variant on purpose. `list_by_root` joins through
/// `nodes.root_id` so the read uses the index; dropping the predicate would
/// mean scanning every capped row in the file, which is exactly the set of
/// rows the per-endpoint cap exists to bound. A caller that wants a
/// different project's history names that project.
#[tauri::command]
pub fn response_history_list(
    state: State<DbPool>,
    root_id: String,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::response_history::list_by_root(c, &root_id)
    })
}

/// Store one response and hand back the row as it actually landed.
///
/// `db::response_history::append` clips the body at
/// `RESPONSE_HISTORY_MAX_BODY_BYTES` and trims the endpoint back to
/// `RESPONSE_HISTORY_PER_ENDPOINT` inside this transaction, so the two
/// caps hold for *every* writer rather than only for the renderer that
/// happens to remember to trim — and the trim cannot be observed
/// half-applied by a concurrent read.
///
/// The returned entry is the stored row, not the request's own copy: after
/// a clip the two disagree, and the renderer swaps this entry into the
/// strip it is already holding. Echoing the request back would show a body
/// the database never kept and a `bodyTruncated` of `false` on a body that
/// was cut.
#[tauri::command]
pub fn response_history_append(
    state: State<DbPool>,
    endpoint_id: String,
    response: StoredResponse,
) -> Result<ResponseHistoryEntry, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::response_history::append(c, &endpoint_id, &response)
    })
}

/// Drop one endpoint's history, answering how many rows went.
///
/// The only caller of `clear` is the explicit "clear this endpoint"
/// affordance — deleting an endpoint or a project takes its history with
/// it through the `endpoint_id` foreign key instead, so there is
/// deliberately no prune call anywhere in a delete path. The count comes
/// back because an endpoint with nothing stored is a different UI state
/// from one that had ten rows, and the caller cannot tell them apart from
/// a `204`.
#[tauri::command]
pub fn response_history_clear(
    state: State<DbPool>,
    endpoint_id: String,
) -> Result<usize, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::response_history::clear(c, &endpoint_id)
    })
}
