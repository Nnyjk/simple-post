// ===========================================================================
// region: spec
// ===========================================================================
//
// 一份接口规格的 Tauri 半边；REST 半边在 `web_api/routes/spec.rs`，两者都过
// `crate::spec_api`，故与 MCP 工具 `spec_upsert` 落的是同一份东西。

use tauri::State;

use crate::db::DbPool;
use crate::error::AppError;
use crate::spec::upsert::EndpointSpec;
use crate::spec_api::SpecUpsertResult;

/// 一次调用落一份完整的接口规格。
///
/// 参数是**一份规格**，不是一堆零散的字段：模块、集合路径上的每一级、环境、服务与
/// 端点都在里面。每一部分都按名字找，找不到就建出来，故反复应用同一份规格不改变
/// 任何东西。
///
/// 返回值摊平：线上的键就是结果自己的键（`projectId` / `module` / `endpoint` …），
/// `missing` 与它们平级 —— 那是「这个端点还缺什么才能发出去」。
///
/// ## 权限
///
/// 本命令**不查**逐项目权限档位：那张表管的是 AI 客户端在这个项目里能做什么。
/// MCP 工具那一条路在落库前要过门；这一条是用户自己的会话，与 `nodes_create` 同族。
#[tauri::command]
pub fn spec_upsert(state: State<DbPool>, spec: EndpointSpec) -> Result<SpecUpsertResult, AppError> {
    crate::spec_api::upsert(&state, &spec)
}

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
