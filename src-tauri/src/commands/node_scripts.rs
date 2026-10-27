// ===========================================================================
// region: node scripts
// ===========================================================================
//
// collection 节点上的两段脚本。REST 半边在 `web_api/routes/node_scripts.rs`，
// 两者都过 `crate::db::node_scripts`，两种传输不可能分叉。
//
// 端点的脚本不在这里：它们住在 `endpoint_details` 行里，随 `endpoint_detail_update`
// 一起改。
//
// patch 整体作为一个参数（而不是逐字段展开）：脚本的「清空」与「不动」靠「字段缺席」与
// 「显式 `null`」区分，而这个区分只有在整个结构体被反序列化时才成立——逐字段展开会让
// Tauri 逐个取值，serde 内建的 `Option<Option<T>>` 把缺席与 `null` 折叠成同一件事。

use tauri::State;

use crate::db::DbPool;
use crate::domain::{NodeScripts, NodeScriptsPatch};
use crate::error::AppError;

/// 读一个节点的脚本。**没有配置过不是错误**：回一份全空的。
#[tauri::command]
pub fn node_scripts_get(state: State<DbPool>, node_id: String) -> Result<NodeScripts, AppError> {
    crate::db::with_conn(&state, |c| crate::db::node_scripts::get(c, &node_id))
}

/// 改一个节点的脚本。只有 collection 能带脚本，别的节点类型会被拒。
#[tauri::command]
pub fn node_scripts_update(
    state: State<DbPool>,
    node_id: String,
    patch: NodeScriptsPatch,
) -> Result<NodeScripts, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::node_scripts::set(c, &node_id, patch.clone())
    })
}
