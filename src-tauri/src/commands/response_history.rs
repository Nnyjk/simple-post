// ===========================================================================
// region: response_history
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::{ResponseHistoryEntry, StoredResponse};
use crate::error::AppError;

/// One endpoint's stored responses, newest first.
///
/// Scoped to a single endpoint on purpose. The first cut of this command
/// took a project and answered "every response in it", which sounded
/// cheaper than a read per endpoint until it was measured against what
/// the strip actually shows: at most `RESPONSE_HISTORY_PER_ENDPOINT` rows
/// for the one endpoint in front of the user. A project-wide read sizes
/// itself to the project, not to the panel, and the renderer has no use
/// for the other endpoints' rows until it opens them.
///
/// So the caller asks for the endpoint it is about to paint, and the
/// renderer keeps one entry per endpoint in a map it already had.
///
/// An unknown id is `Ok(vec![])`, not a 404: the most common request here
/// is the first one for an endpoint that has never been sent, and that is
/// an empty history rather than a missing resource.
#[tauri::command]
pub fn response_history_list(
    state: State<DbPool>,
    endpoint_id: String,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::response_history::list_by_endpoint(c, &endpoint_id)
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
