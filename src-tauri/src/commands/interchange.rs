// ===========================================================================
// region: interchange
// ===========================================================================

//! 导入 / 导出的 Tauri 入口。四个命令全是薄壳：逻辑在
//! [`crate::interchange_api`]，REST 侧（`web_api/routes/interchange.rs`）调的是同一批
//! 函数。把它们写在这里而不是各自实现，是 AGENTS.md dual-transport invariant 的要求——
//! 少一侧不会报错，只会静默少一半功能。
//!
//! 线格式在 [`crate::dto`]：两侧共用同一组 DTO，故「桌面发出去的 body」与「浏览器发
//! 出去的 body」逐字节相同，`src/lib/tauri-commands.ts` 的双派发才不用分叉。

use tauri::State;

use crate::db::DbPool;
use crate::dto::{
    ExportDocumentDto, ExportDocumentOpts, ExportPreviewDto, ExportPreviewOpts, ImportCommitDto,
    ImportCommitOpts, ImportPreviewDto, ImportPreviewOpts,
};
use crate::error::AppError;

/// 解析一份外来文档并给出可勾选的中间态。**不写库。**
///
/// URL 分支要发请求，故是 `async`；文件分支不发，但两种来源共用同一条路径，
/// 免得「本地文件」与「远程链接」走出两套解析规则。
#[tauri::command]
pub async fn interchange_import_preview(
    state: State<'_, DbPool>,
    opts: ImportPreviewOpts,
) -> Result<ImportPreviewDto, AppError> {
    crate::interchange_api::import_preview(&state, &opts).await
}

/// 把勾中的条目落进目标模块。树与 Postman 环境在**同一个事务**里。
#[tauri::command]
pub fn interchange_import_commit(
    state: State<'_, DbPool>,
    opts: ImportCommitOpts,
) -> Result<ImportCommitDto, AppError> {
    crate::interchange_api::import_commit(&state, &opts)
}

/// 导出候选清单。**不写库。**
#[tauri::command]
pub fn interchange_export_preview(
    state: State<'_, DbPool>,
    opts: ExportPreviewOpts,
) -> Result<ExportPreviewDto, AppError> {
    crate::interchange_api::export_preview(&state, &opts)
}

/// 按选定的环境与勾选的条目产出 OpenAPI 文档。**不写库。**
///
/// 写盘不在这条命令里：路径由 `tauri-plugin-dialog` 的保存对话框决定，那是渲染侧的
/// 选择；命令只负责把文档本身交回去。
#[tauri::command]
pub fn interchange_export_document(
    state: State<'_, DbPool>,
    opts: ExportDocumentOpts,
) -> Result<ExportDocumentDto, AppError> {
    crate::interchange_api::export_document(&state, &opts)
}
