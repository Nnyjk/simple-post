//! 端点配置一组（`/api/endpoint-details` 与 `/api/nodes/{id}/…` 下的配置动词）。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/endpoint-details",
            summary: "Every endpoint's request configuration, flat",
            stability: "public",
            request: None,
            response: Some("EndpointDetailList"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/nodes/{id}/endpoint-detail",
            summary: "Get an endpoint's request configuration",
            stability: "public",
            request: None,
            response: Some("EndpointDetail"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/nodes/{id}/endpoint-detail",
            summary: "Partially update an endpoint's request configuration",
            stability: "public",
            request: Some("EndpointDetailPatch"),
            response: Some("EndpointDetail"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/nodes/{id}/scripts",
            summary: "A collection's pre-request and test scripts; a collection without \
                      scripts answers an empty pair rather than 404",
            stability: "public",
            request: None,
            response: Some("NodeScripts"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/nodes/{id}/scripts",
            summary: "Partially update a collection's pre-request and test scripts",
            stability: "public",
            request: Some("NodeScriptsPatch"),
            response: Some("NodeScripts"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/nodes/{id}/duplicate",
            summary: "Copy an endpoint (endpoints only); an optional newId names the copy",
            stability: "public",
            request: Some("NodeDuplicate"),
            response: Some("ResourceNode"),
        },
    ]
}
