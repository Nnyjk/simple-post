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
//!
//! **命令参数是扁平的，不是 `opts: ImportPreviewOpts` 一个包装。** `#[tauri::command]`
//! 按参数名去 payload 里取值，而 REST 侧 `Json<ImportPreviewOpts>` 直接吃整个 body；
//! 渲染层两边发的是同一个扁平对象，于是参数名必须就是那些 camelCase 字段名本身。
//! 取 `opts:` 包装的话桌面侧会报 `missing required key opts`——只坏桌面、不坏浏览器，
//! 正是这条不变量想消灭的那类半失效。同样的形状见 [`crate::commands::oauth2_token`]。

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
    project_id: String,
    module_id: String,
    content: Option<String>,
    url: Option<String>,
) -> Result<ImportPreviewDto, AppError> {
    crate::interchange_api::import_preview(
        &state,
        &ImportPreviewOpts {
            project_id,
            module_id,
            content,
            url,
        },
    )
    .await
}

/// 把勾中的条目落进目标模块。树与 Postman 环境在**同一个事务**里。
#[tauri::command]
pub fn interchange_import_commit(
    state: State<'_, DbPool>,
    project_id: String,
    module_id: String,
    content: String,
    selected: Option<Vec<String>>,
) -> Result<ImportCommitDto, AppError> {
    crate::interchange_api::import_commit(
        &state,
        &ImportCommitOpts {
            project_id,
            module_id,
            content,
            selected: selected.unwrap_or_default(),
        },
    )
}

/// 导出候选清单。**不写库。**
#[tauri::command]
pub fn interchange_export_preview(
    state: State<'_, DbPool>,
    project_id: String,
    module_id: String,
) -> Result<ExportPreviewDto, AppError> {
    crate::interchange_api::export_preview(
        &state,
        &ExportPreviewOpts {
            project_id,
            module_id,
        },
    )
}

/// 按选定的环境与勾选的条目产出 OpenAPI 文档。**不写库。**
///
/// 写盘不在这条命令里：路径由 `tauri-plugin-dialog` 的保存对话框决定，那是渲染侧的
/// 选择；命令只负责把文档本身交回去。
#[tauri::command]
pub fn interchange_export_document(
    state: State<'_, DbPool>,
    project_id: String,
    module_id: String,
    environment_id: Option<String>,
    selected: Option<Vec<String>>,
    format: String,
) -> Result<ExportDocumentDto, AppError> {
    crate::interchange_api::export_document(
        &state,
        &ExportDocumentOpts {
            project_id,
            module_id,
            environment_id: environment_id.unwrap_or_default(),
            selected: selected.unwrap_or_default(),
            format,
        },
    )
}

#[cfg(test)]
#[path = "interchange_test.rs"]
mod interchange_test;
