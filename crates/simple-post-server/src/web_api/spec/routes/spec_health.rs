//! `/api/healthz` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![RouteSpec {
        method: "GET",
        path: "/api/healthz",
        summary: "Liveness probe (no auth)",
        stability: "public",
        request: None,
        response: Some("HealthzResponse"),
    }]
}
