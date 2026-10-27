//! `/api/spec/upsert` 一组。
//!
//! 一次调用落一整份规格：模块、集合路径上的每一级、环境、服务与端点。
//! 与 MCP 工具 `spec_upsert` 落的是同一份东西（逻辑在 `crate::spec_api`），
//! 故请求体的形状与那个工具的参数逐字段相同。
//!
//! 字段名两种拼写都收：`project_id` 与 `projectId` 都行 —— 前者是 MCP 那条线
//! 的惯例，后者是这条线通篇用的 camelCase，两者落在同一个类型上。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![RouteSpec {
        method: "POST",
        path: "/api/spec/upsert",
        summary: "Apply one interface spec in a single call: the module, the \
                      collections on its path, the environment, the service and the \
                      endpoint. Each part is found by name and created when missing, \
                      so a second call reuses them all; only if_exists=duplicate adds \
                      a second endpoint",
        stability: "public",
        request: Some("EndpointSpec"),
        response: Some("SpecUpsertResult"),
    }]
}
