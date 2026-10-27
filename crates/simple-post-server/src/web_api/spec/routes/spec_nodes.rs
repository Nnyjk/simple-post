//! 资源树的结构动词一组（`/api/nodes` 与 `/api/nodes/move`）。
//!
//! 结构更新与 endpoint 配置刻意分成两条路由：一次改名不该重写那些根本没变的
//! JSON 列。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/nodes",
            summary: "List tree nodes; filter by rootId, type, or search q",
            stability: "public",
            request: None,
            response: Some("ResourceNode[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/nodes",
            summary: "Create a node (an endpoint carries its detail in the same body)",
            stability: "public",
            request: Some("NodeCreate"),
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/nodes/{id}",
            summary: "Get one node",
            stability: "public",
            request: None,
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/nodes/{id}",
            summary: "Partially update a node's structural fields",
            stability: "public",
            request: Some("ResourceNodePatch"),
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/nodes/{id}",
            summary: "Delete a node and its whole subtree",
            stability: "public",
            request: None,
            response: None,
        },
        RouteSpec {
            method: "POST",
            path: "/api/nodes/move",
            summary: "Move nodes to a new parent and slot",
            stability: "public",
            request: Some("NodeMove"),
            response: None,
        },
    ]
}
