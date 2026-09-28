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
        // ---- modules ----------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/modules",
            summary: "List modules in a project",
            stability: "public",
            request: None,
            response: Some("Module[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/modules",
            summary: "Create a module",
            stability: "public",
            request: Some("CreateModule"),
            response: Some("Module"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/modules/:id",
            summary: "Patch a module",
            stability: "public",
            request: Some("ModulePatch"),
            response: Some("Module"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/modules/:id",
            summary: "Delete a module",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/modules/move",
            summary: "Move a module within a project",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Module"),
        },
        // ---- collections ------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/collections",
            summary: "List collections",
            stability: "public",
            request: None,
            response: Some("Collection[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/collections",
            summary: "Create a collection",
            stability: "public",
            request: Some("CreateCollection"),
            response: Some("Collection"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/collections/:id",
            summary: "Patch a collection",
            stability: "public",
            request: Some("CollectionPatch"),
            response: Some("Collection"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/collections/:id",
            summary: "Delete a collection",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/collections/move",
            summary: "Move a collection",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Collection"),
        },
        // ---- endpoints --------------------------------------------------
        RouteSpec {
            method: "GET",
            path: "/api/endpoints",
            summary: "List endpoints",
            stability: "public",
            request: None,
            response: Some("Endpoint[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints",
            summary: "Create an endpoint",
            stability: "public",
            request: Some("CreateEndpoint"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "GET",
            path: "/api/endpoints/:id",
            summary: "Get one endpoint",
            stability: "public",
            request: None,
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/endpoints/:id",
            summary: "Patch an endpoint",
            stability: "public",
            request: Some("EndpointPatch"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/endpoints/:id",
            summary: "Delete an endpoint",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/search",
            summary: "Search endpoints by name/method/url",
            stability: "public",
            request: Some("SearchQuery"),
            response: Some("Endpoint[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/move",
            summary: "Move an endpoint",
            stability: "public",
            request: Some("MoveTarget"),
            response: Some("Endpoint"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/:id/duplicate",
            summary: "Duplicate an endpoint",
            stability: "public",
            request: None,
            response: Some("Endpoint"),
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
        RouteSpec {
            method: "POST",
            path: "/api/endpoints/:id/send",
            summary: "Send a stored endpoint by id; the server resolves the \
                      base URL, path, variables and auth",
            stability: "public",
            request: Some("EndpointSendArgs"),
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
