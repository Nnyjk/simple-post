//! `/api/projects/{projectId}/baseurls` 与 `/api/baseurl-defs/{defId}` 一组。
//!
//! 服务属于项目，故读与两个写挂在 `/api/projects/{projectId}/baseurls`。
//! 只有按 id 的动词留在 `/api/baseurl-defs/{defId}`：改名或删除点名的是服务
//! 本身，不需要项目上下文就能找到它。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/projects/{projectId}/baseurls",
            summary: "List a project's services; every module can resolve every one",
            stability: "public",
            request: None,
            response: Some("BaseUrlDefinition[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/{projectId}/baseurls",
            summary: "Add a service to the project",
            stability: "public",
            request: Some("BaseUrlDefCreate"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/{projectId}/baseurls/default",
            summary:
                "Set (or clear) the project's star default service; answers the updated Project",
            stability: "public",
            request: Some("BaseUrlDefSetDefault"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/baseurl-defs/{defId}",
            summary: "Rename or re-describe a service",
            stability: "public",
            request: Some("BaseUrlDefPatch"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/baseurl-defs/{defId}",
            summary: "Delete a service; the star pointer is cleared with it if it named this one",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
    ]
}
