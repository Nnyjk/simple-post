//! `/api/projects` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/projects",
            summary: "List projects",
            stability: "public",
            request: None,
            response: Some("Project[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects",
            summary: "Create a project (body: name plus an opts object, or a flat Project)",
            stability: "public",
            request: Some("Project"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/projects/{id}",
            summary: "Get one project",
            stability: "public",
            request: None,
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/projects/{id}",
            summary: "Patch a project",
            stability: "public",
            request: Some("ProjectPatch"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/projects/{id}",
            summary: "Delete a project",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
    ]
}
