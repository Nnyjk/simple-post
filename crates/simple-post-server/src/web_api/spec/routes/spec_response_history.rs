//! `/api/response-history` 一组。
//!
//! 登记它们是因为浏览器端要用：三个动词与 `/api/endpoint-details` 的读
//! 一起构成响应历史条的持久化面。
//!
//! 列表与清除共用一条路径，因为作用对象都是「某个 endpoint 的若干行」；
//! 只有追加是集合级的，它点的 id 在 body 里，见 `routes/response_history.rs`。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/response-history/{endpointId}",
            summary: "Stored responses for one endpoint; newest first, at most the endpoint's \
                      10 kept rows. An endpoint with nothing stored answers []",
            stability: "public",
            request: None,
            response: Some("ResponseHistoryEntry[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/response-history",
            summary: "Store one response; the server clips the body at 256 KiB and trims the endpoint back to its 10 newest rows",
            stability: "public",
            request: Some("ResponseHistoryAppend"),
            response: Some("ResponseHistoryEntry"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/response-history/{endpointId}",
            summary: "Clear one endpoint's stored responses; answers how many rows were removed",
            stability: "public",
            request: None,
            response: Some("DeleteCount"),
        },
    ]
}
