// ===========================================================================
// region: response_history
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::{ResponseHistoryEntry, StoredRequestSnapshot, StoredResponse};
use crate::error::AppError;

/// 某个 endpoint 的响应，最新的在前。
///
/// 刻意按 endpoint 而非按项目：面板一次只画当前这一个；按项目读则要按项目规模来，而
/// 其余 endpoint 的行在打开它们之前没人用。
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
/// `RESPONSE_HISTORY_MAX_BODY_BYTES`、把该 endpoint 的**未收藏**历史裁到保留条数：
/// 两个上限对**每个**写入方都成立，而不只对恰好记得裁剪的那个渲染侧成立，且并发读
/// 看不到裁剪只做了一半的中间态。
///
/// `request` 是那次发送出去的请求（解析之后的 URL / header / body）。它缺席时行里
/// 就没有请求侧栏可画——那正是写这一列之前就有的那些行的样子。
///
/// 返回的是库里的行而非请求自己的副本：截断后两者已不一致，而渲染侧要把这条换进它
/// 手里已有的列表；回显请求会显示数据库从未保留的 body，以及在被截的 body 上标一个
/// `bodyTruncated` 为 `false`。
#[tauri::command]
pub fn response_history_append(
    state: State<DbPool>,
    endpoint_id: String,
    response: StoredResponse,
    request: Option<StoredRequestSnapshot>,
) -> Result<ResponseHistoryEntry, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::response_history::append(c, &endpoint_id, &response, request.as_ref())
    })
}

/// 清空某个 endpoint 的**未收藏**历史，回报删掉几行。
///
/// 收藏的行留着：用户说它们永久保存，故这个动作只清自动保留的那部分。删掉一行收藏
/// 有它自己的入口——取消收藏。
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

/// 收藏或取消收藏一条历史。返回这一行是否存在。
///
/// 收藏状态住在行上，故它跟着那条 endpoint 走：删掉 endpoint，外键带走的正是它。
#[tauri::command]
pub fn response_history_set_favorite(
    state: State<DbPool>,
    id: String,
    favorite: bool,
) -> Result<bool, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::response_history::set_favorite(c, &id, favorite)
    })
}

/// 读「每个 endpoint 保留多少条未收藏历史」。
///
/// 全局设置，住在 `preferences` 的 `response_history.keep` 行里；库里的裁剪与界面上的
/// 列表都按它来，故这不是一份显示用的副本。
#[tauri::command]
pub fn response_history_keep_get(state: State<DbPool>) -> Result<i64, AppError> {
    crate::db::with_conn(&state, crate::db::preferences::response_history_keep)
}

/// 改那个保留条数，并立刻按新条数裁一遍库里已有的行。
///
/// 立刻裁是必要的：否则用户把条数从 50 调到 10，要等到下一次发送才会看到历史变短，
/// 而那条历史的条数是用户自己刚改的设置。
#[tauri::command]
pub fn response_history_keep_set(state: State<DbPool>, keep: i64) -> Result<i64, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::preferences::set_response_history_keep(c, keep)?;
        let applied = crate::db::preferences::response_history_keep(c)?;
        crate::db::response_history::trim_all(c, applied)?;
        Ok(applied)
    })
}
