// ===========================================================================
// region: response_history
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::{ResponseHistoryEntry, StoredResponse};
use crate::error::AppError;

/// 某个 endpoint 的响应，最新在前。
///
/// 刻意按 endpoint 而非按项目：面板一次只画当前这一个，最多
/// `RESPONSE_HISTORY_PER_ENDPOINT` 行；按项目读则要按项目规模来，而其余 endpoint 的行
/// 在打开它们之前没人用。
///
/// 未知 id 返回 `Ok(vec![])` 而非 404：这里最常见的请求就是一个从未发送过的 endpoint
/// 的首次请求，那是空历史，不是资源缺失。
#[tauri::command]
pub fn response_history_list(
    state: State<DbPool>,
    endpoint_id: String,
) -> Result<Vec<ResponseHistoryEntry>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::response_history::list_by_endpoint(c, &endpoint_id)
    })
}

/// 存一条响应，并交回它实际落库后的样子。
///
/// `db::response_history::append` 在此事务内把 body 截到
/// `RESPONSE_HISTORY_MAX_BODY_BYTES`、把该 endpoint 裁到
/// `RESPONSE_HISTORY_PER_ENDPOINT`：两个上限对**每个**写入方都成立，而不只对恰好记得
/// 裁剪的那个渲染侧成立，且并发读看不到裁剪只做了一半的中间态。
///
/// 返回的是库里的行而非请求自己的副本：截断后两者已不一致，而渲染侧要把这条换进它
/// 手里已有的列表；回显请求会显示数据库从未保留的 body，以及在被截的 body 上标一个
/// `bodyTruncated` 为 `false`。
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

/// 清空某个 endpoint 的历史，回报删掉几行。
///
/// `clear` 的唯一调用方是「清空当前 endpoint」这个显式操作——删 endpoint 或删项目时
/// 历史由 `endpoint_id` 外键带走，所以删除路径里刻意没有任何 prune 调用。返回行数是
/// 因为「本来就没有」和「本来有十行」是两种 UI 状态，而调用方从 `204` 分不出。
#[tauri::command]
pub fn response_history_clear(
    state: State<DbPool>,
    endpoint_id: String,
) -> Result<usize, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::response_history::clear(c, &endpoint_id)
    })
}
