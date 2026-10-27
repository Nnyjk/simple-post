//! 一份接口规格的 REST handler。
//!
//! 一条路由：`POST /api/spec/upsert`。逻辑全部在 `crate::spec_api`，故 Tauri 命令与
//! 这条路由走同一套实现，且与 MCP 工具 `spec_upsert` 落库落的是同一份东西。
//!
//! ## 请求体就是那份规格
//!
//! 没有外层包装：body 的形状与 MCP 工具 `spec_upsert` 的参数**逐字段相同**，故一份
//! 规格可以从 MCP 客户端原样搬到 HTTP 调用方，不必翻译。
//!
//! 字段名两种拼写都收：`project_id` 与 `projectId` 都行。前者是 MCP 那条线的惯例，
//! 后者是这条线（以及整个应用）通篇用的 camelCase，两者落在**同一个**类型上。
//!
//! ## 不查逐项目权限档位
//!
//! `permissions` 表管的是「AI 客户端在这个项目里能做什么」，故 MCP 工具落库前要过
//! 那道门。这条路由是用户自己的会话，与 `nodes_create` 同一族，故不查。

use std::sync::Arc;

use axum::{extract::State, Json};
use serde_json::Value;

use crate::error::AppError;
use crate::spec_api::SpecUpsertResult;

use super::super::{map_app_error, HttpErrorResponse, WebState};

/// `POST /api/spec/upsert` —— 一次调用落一份完整的接口规格。
///
/// 回的是「落了什么」与「端点还缺什么」两半：模块、集合路径上的每一级、环境、服务
/// 与端点各自的 id，以及这个端点还缺哪几样才能发出去。
pub(crate) async fn spec_upsert(
    State(s): State<Arc<WebState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpErrorResponse> {
    // 手动反序列化而不是让 axum 收 `Json<EndpointSpec>`：规格写错时（多一个键、
    // 少一个必填项）错误信息就是这次调用最该看见的东西，故它走应用自己的错误信封
    // （400 + `{code, message}`），而不是 axum 的 422 纯文本。
    let spec: crate::spec::upsert::EndpointSpec = serde_json::from_value(body).map_err(|e| {
        map_app_error(AppError::Validation(format!(
            "spec_upsert 的规格反序列化失败：{e}"
        )))
    })?;

    let result: SpecUpsertResult =
        crate::spec_api::upsert(&s.pool, &spec).map_err(map_app_error)?;
    Ok(Json(serde_json::to_value(&result).unwrap_or(Value::Null)))
}

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
