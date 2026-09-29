// =========================================================================
// Public HTTP API contract (commit 2: chore(docs): HTTP API 公开契约)
// =========================================================================
//
// `RouteSpec` is the single source of truth for which `/api/*` routes
// are part of the stable contract. `scripts/dump-openapi.mjs` reads this
// list (plus the corresponding `WEB_ROUTES` entries in
// `src/lib/tauri-commands.ts`) to generate `docs/openapi.yaml`. Anything
// NOT in `public_route_specs()` is treated as `internal` — not part of
// the stable v1 contract, no backward-compat promise.
//
// Stability rules:
//   * `public`   — endpoint shape, auth, and response codes are stable.
//                  Breaking changes require a `/api/v2/*` migration.
//   * `internal` — no version commitment; UI / supervisor use only.
//
// This list is hand-curated and intentionally does NOT enumerate every
// axum router binding (e.g. `/mcp/*` from `daemon_http` are out of
// scope — they're a JSON-RPC MCP contract, documented separately).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RouteSpec {
    /// HTTP method (GET / POST / PUT / PATCH / DELETE).
    pub method: &'static str,
    /// Path template (axum syntax: `/api/projects/:id`).
    pub path: &'static str,
    /// Short description for the OpenAPI doc.
    pub summary: &'static str,
    /// `'public'` part of the stable v1 contract, or `'internal'`.
    pub stability: &'static str,
    /// Optional request body shape name (looked up by the OpenAPI dumper).
    /// `None` means "no body" or "shape not yet documented".
    pub request: Option<&'static str>,
    /// Optional response shape name. `None` means "shape not yet
    /// documented" — the dumper emits an `unknown` placeholder.
    pub response: Option<&'static str>,
}

/// Single source of truth for the public HTTP API. **Hand-curated** —
/// any new endpoint that should be in the v1 stable contract MUST be
/// added here. Anything absent is `internal` and not part of OpenAPI.
pub fn public_route_specs() -> Vec<RouteSpec> {
    vec![
        // ---- health -----------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/healthz",
            summary: "Liveness probe (no auth)",
            stability: "public",
            request: None,
            response: Some("HealthzResponse"),
        },
        // ---- projects ---------------------------------------------------
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
            summary: "Create a project",
            stability: "public",
            request: Some("CreateProject"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/projects/:id",
            summary: "Get one project",
            stability: "public",
            request: None,
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/projects/:id",
            summary: "Patch a project",
            stability: "public",
            request: Some("ProjectPatch"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/projects/:id",
            summary: "Delete a project",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        // ---- resource tree ---------------------------------------------
        // Replaces the 19 /api/modules/*, /api/collections/* and
        // /api/endpoints/* paths these used to be spread across. The
        // structural update and the endpoint configuration are separate
        // routes on purpose: a rename must not rewrite JSON columns that
        // did not change.
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
            path: "/api/nodes/:id",
            summary: "Get one node",
            stability: "public",
            request: None,
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/nodes/:id",
            summary: "Partially update a node's structural fields",
            stability: "public",
            request: Some("ResourceNodePatch"),
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/nodes/:id",
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
        RouteSpec {
            method: "GET",
            path: "/api/nodes/:id/endpoint-detail",
            summary: "Get an endpoint's request configuration",
            stability: "public",
            request: None,
            response: Some("EndpointDetail"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/nodes/:id/endpoint-detail",
            summary: "Partially update an endpoint's request configuration",
            stability: "public",
            request: Some("EndpointDetailPatch"),
            response: Some("EndpointDetail"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/nodes/:id/duplicate",
            summary: "Copy an endpoint (endpoints only)",
            stability: "public",
            request: None,
            response: Some("ResourceNode"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/nodes/:id/send",
            summary: "Send a stored endpoint by id; the caller never names a URL",
            stability: "public",
            request: Some("EndpointSend"),
            response: Some("HttpResult"),
        },
        // ---- baseUrl definitions ----------------------------------------
        // All five were live but undocumented. They are project-scoped
        // for reads / creates / set-default and id-scoped for mutations,
        // matching the two route families the handlers comment describes.
        RouteSpec {
            method: "GET",
            path: "/api/projects/:projectId/baseurls",
            summary: "List a project's base URL definitions",
            stability: "public",
            request: None,
            response: Some("BaseUrlDefinition[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls",
            summary: "Create a base URL definition in a project",
            stability: "public",
            request: Some("BaseUrlDefinition"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls/default",
            summary: "Set (or clear) the project's default base URL definition",
            stability: "public",
            request: Some("BaseUrlDefSetDefault"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/baseurl-defs/:defId",
            summary: "Rename or re-describe a base URL definition",
            stability: "public",
            request: Some("BaseUrlDefPatch"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/baseurl-defs/:defId",
            summary: "Delete a base URL definition",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        // ---- environments -----------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/environments",
            summary: "List environments in a project",
            stability: "public",
            request: None,
            response: Some("Environment[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments",
            summary: "Create an environment",
            stability: "public",
            request: Some("CreateEnvironment"),
            response: Some("Environment"),
        },
        // ---- http send --------------------------------------------------
        RouteSpec {
            method: "POST",
            path: "/api/http/send",
            summary: "Execute an arbitrary HTTP request (raw-URL escape hatch; \
                      honours the outbound URL policy)",
            stability: "public",
            request: Some("HttpRequestInput"),
            response: Some("HttpResult"),
        },
        // ---- internal (not in OpenAPI v1) -------------------------------
        // The following endpoints are intentionally NOT listed above —
        // they are supervisor / UI projections and have no version
        // commitment. They appear in `WEB_ROUTES` for runtime access but
        // the OpenAPI v1 doc omits them.
        //   GET  /api/mcp/status             - daemon status (UI)
        //   POST /api/mcp/start              - daemon launch
        //   POST /api/mcp/stop               - daemon stop
        //   POST /api/mcp/restart            - daemon restart
        //   GET  /api/mcp/proxy-path         - mcp_proxy binary path
        //   GET  /api/mcp/capability         - capability policy (commit 1)
        //   PUT  /api/mcp/capability         - capability policy (commit 1)
        //   GET  /api/mcp/capability/tools   - visible tools under current policy
        //   GET  /api/preferences            - raw preferences dump (UI)
        //   PUT  /api/preferences            - raw preferences patch
        // /api/permissions* and /api/request_settings* are intentionally
        // absent from the public contract. The baseUrl-def family is now
        // documented above.
    ]
}
