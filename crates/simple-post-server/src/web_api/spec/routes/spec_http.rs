//! `/api/http/send` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![RouteSpec {
        method: "POST",
        path: "/api/http/send",
        summary: "Execute an arbitrary HTTP request (raw-URL escape hatch; \
                      honours the outbound URL policy)",
        stability: "public",
        request: Some("HttpRequestInput"),
        response: Some("HttpResult"),
    }]
}
