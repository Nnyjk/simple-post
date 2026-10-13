// =========================================================================
// Public HTTP API contract
// =========================================================================
//
// `RouteSpec` is the single source of truth for which `/api/*` routes
// are part of the stable contract. `scripts/dump-openapi.mjs` reads this
// list (plus the corresponding `WEB_ROUTES` entries in
// `src/lib/tauri-commands.ts`) to generate an OpenAPI document. Anything
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
            summary: "Create a project (body: name plus an opts object, or a flat Project)",
            stability: "public",
            request: Some("Project"),
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
            path: "/api/endpoint-details",
            summary: "Every endpoint's request configuration, flat",
            stability: "public",
            request: None,
            response: Some("EndpointDetailList"),
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
            summary: "Copy an endpoint (endpoints only); an optional newId names the copy",
            stability: "public",
            request: Some("NodeDuplicate"),
            response: Some("ResourceNode"),
        },
        // ---- baseUrl definitions ----------------------------------------
        // A service belongs to the project, so the read and both writes
        // hang off `/api/projects/:projectId/baseurls`. Only the id-scoped
        // verbs stay under `/api/baseurl-defs/:defId`, because a rename or
        // a delete names the service itself and needs no project context.
        RouteSpec {
            method: "GET",
            path: "/api/projects/:projectId/baseurls",
            summary: "List a project's services; every module can resolve every one",
            stability: "public",
            request: None,
            response: Some("BaseUrlDefinition[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls",
            summary: "Add a service to the project",
            stability: "public",
            request: Some("BaseUrlDefCreate"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/projects/:projectId/baseurls/default",
            summary:
                "Set (or clear) the project's star default service; answers the updated Project",
            stability: "public",
            request: Some("BaseUrlDefSetDefault"),
            response: Some("Project"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/baseurl-defs/:defId",
            summary: "Rename or re-describe a service",
            stability: "public",
            request: Some("BaseUrlDefPatch"),
            response: Some("BaseUrlDefinition"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/baseurl-defs/:defId",
            summary: "Delete a service; the star pointer is cleared with it if it named this one",
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
        RouteSpec {
            method: "PATCH",
            path: "/api/environments/:envId",
            summary: "Update an environment (null clears a field)",
            stability: "public",
            request: Some("EnvironmentPatch"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/environments/:envId",
            summary: "Delete an environment",
            stability: "public",
            request: None,
            response: Some("DeleteAck"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/active",
            summary: "Set the active environment for a project",
            stability: "public",
            request: Some("SetActiveEnv"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/:envId/vars",
            summary: "Set one environment variable (null deletes it)",
            stability: "public",
            request: Some("EnvSetVar"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/:envId/vars/rename",
            summary: "Rename an environment variable in one step",
            stability: "public",
            request: Some("EnvRenameVar"),
            response: Some("Environment"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/environments/:envId/module-baseurls",
            summary: "Set one service's URL for one module in this environment \
                      (the only layer; null clears that one cell)",
            stability: "public",
            request: Some("EnvSetModuleBaseUrl"),
            response: Some("Environment"),
        },
        // ---- response history -------------------------------------------
        // Listed because the browser build needs it: the response-history
        // strip is renderer state that used to be lost on every relaunch,
        // and these three routes are the second half of the fix — the
        // first half is the `/api/endpoint-details` read they are
        // rendered next to.
        //
        // The list and the clear share one path because they act on the
        // same unit — a single endpoint's rows. Only the append is
        // collection-level, because the id it names is in its body
        // alongside the response; see `routes/response_history.rs`.
        RouteSpec {
            method: "GET",
            path: "/api/response-history/:endpointId",
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
            path: "/api/response-history/:endpointId",
            summary: "Clear one endpoint's stored responses; answers how many rows were removed",
            stability: "public",
            request: None,
            response: Some("DeleteCount"),
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
        // ---- mcp daemon lifecycle (manual switch) ----------------------
        // Documented rather than left internal: `start` / `stop` are the
        // renderer-facing ON/OFF switch for the embedded daemon, they
        // share the Tauri commands `mcp_daemon_start` /
        // `mcp_daemon_stop`, and they are the pair a third-party client
        // needs in order to control the daemon at runtime. Both answer
        // with the same `{ ok, pid, port }` envelope as `/restart`.
        RouteSpec {
            method: "POST",
            path: "/api/mcp/start",
            summary: "Start the embedded MCP daemon (idempotent: a healthy \
                      running daemon is left untouched and echoed back)",
            stability: "public",
            request: None,
            response: Some("RestartResult"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/mcp/stop",
            summary: "Stop the embedded MCP daemon and release its port file \
                      (idempotent; always `{ ok: true, pid: null, port: null }` \
                      on success)",
            stability: "public",
            request: None,
            response: Some("RestartResult"),
        },
        // ---- internal (not in OpenAPI v1) -------------------------------
        // The following endpoints are intentionally NOT listed above —
        // they are supervisor / UI projections and have no version
        // commitment. They appear in `WEB_ROUTES` for runtime access but
        // the OpenAPI v1 doc omits them.
        //   GET  /api/mcp/status             - daemon status (UI)
        //   POST /api/mcp/restart            - daemon restart
        //   GET  /api/mcp/proxy-path         - mcp_proxy binary path
        //   GET  /api/mcp/capability         - capability policy
        //   PUT  /api/mcp/capability         - capability policy
        //   GET  /api/mcp/capability/tools   - visible tools under current policy
        //   GET  /api/mcp/http-config        - daemon bind host/port/path + allowlists
        //   PUT  /api/mcp/http-config        - daemon listener configuration
        //   GET  /api/preferences            - raw preferences dump (UI)
        //   PUT  /api/preferences            - raw preferences patch
        // /api/permissions* and /api/request_settings* are intentionally
        // absent from the public contract. The baseUrl-def family is now
        // documented above.
    ]
}
