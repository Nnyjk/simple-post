//! `/api/environments` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/environments",
            summary: "List environments in a project",
            request: None,
            response: Some("Environment[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments",
            summary: "Create an environment",
            request: Some("CreateEnvironment"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/environments/{envId}",
            summary: "Update an environment (null clears a field)",
            request: Some("EnvironmentPatch"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/environments/{envId}",
            summary: "Delete an environment",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/{envId}/vars",
            summary: "Set one environment variable (null deletes it)",
            request: Some("EnvSetVar"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/{envId}/vars/disabled",
            summary: "Disable or enable one environment variable; a disabled one is \
                      stored and shown but does not take part in variable substitution",
            request: Some("EnvSetVarDisabled"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/{envId}/vars/rename",
            summary: "Rename an environment variable in one step",
            request: Some("EnvRenameVar"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/{envId}/module-baseurls",
            summary: "Set one module's address and proxy in this environment \
                      (body: moduleId, url, proxy; null keeps that field unchanged, \
                      an empty string clears it)",
            request: Some("EnvSetModuleBaseUrl"),
            response: Some("Environment"),
        },
    ]
}
