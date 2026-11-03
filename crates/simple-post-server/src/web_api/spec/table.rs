//! 契约表的汇总入口。
//!
//! `public_route_specs()` 按资源分组依次收集 `routes/` 下各组的行；组的先后
//! 就是原表的先后。

use super::routes;
use super::RouteSpec;

/// 公共 HTTP API 的唯一事实来源。**手工维护**——任何应进入 v1 稳定契约的新
/// 端点都必须在此登记。缺席者按 `internal` 处理，不进 OpenAPI。
///
/// 当前缺席的端点逐条列在 `spec/mod.rs` 的「internal (not in OpenAPI v1)」一节里。
pub fn public_route_specs() -> Vec<RouteSpec> {
    let mut specs = Vec::with_capacity(46);
    specs.extend(routes::spec_health::routes());
    specs.extend(routes::spec_projects::routes());
    specs.extend(routes::spec_nodes::routes());
    specs.extend(routes::spec_endpoint_details::routes());
    specs.extend(routes::spec_dynamic_variables::routes());
    specs.extend(routes::spec_environments::routes());
    specs.extend(routes::spec_response_history::routes());
    specs.extend(routes::spec_interchange::routes());
    specs.extend(routes::spec_http::routes());
    specs.extend(routes::spec_oauth2::routes());
    specs.extend(routes::spec_upsert::routes());
    specs.extend(routes::spec_mcp::routes());
    specs
}
