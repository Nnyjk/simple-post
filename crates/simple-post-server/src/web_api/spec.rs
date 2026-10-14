// =========================================================================
// 公共 HTTP API 契约
// =========================================================================
//
// `RouteSpec` 是「哪些 `/api/*` 路由属于稳定契约」的唯一事实来源。
// `scripts/dump-openapi.mjs` 读这份表（配合 `src/lib/tauri-commands.ts` 的
// `WEB_ROUTES`）生成 OpenAPI 文档。不在 `public_route_specs()` 里的一律按
// `internal` 处理——不在 v1 稳定契约内，无向后兼容承诺。
//
// 稳定性分级：
//   * `public`   —— 端点形状、鉴权、响应码稳定。破坏性变更需走 `/api/v2/*` 迁移。
//   * `internal` —— 无版本承诺，只供 UI / supervisor 使用。
//
// 本表为手工维护，**故意**不枚举每一条 axum 路由绑定（如 `/mcp/*` 属于另一套
// JSON-RPC MCP 契约，另行记录）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RouteSpec {
    /// HTTP 方法（GET / POST / PUT / PATCH / DELETE）。
    pub method: &'static str,
    /// 路径模板（axum 语法：`/api/projects/:id`）。
    pub path: &'static str,
    /// OpenAPI 文档里的简短说明。
    pub summary: &'static str,
    /// `'public'` 属 v1 稳定契约，`'internal'` 不是。
    pub stability: &'static str,
    /// 可选的请求体形状名（由 OpenAPI dumper 查表）。`None` 表示无请求体，
    /// 或该形状尚未记录。
    pub request: Option<&'static str>,
    /// 可选的响应形状名。`None` 会让 dumper 输出 `unknown` 占位。
    pub response: Option<&'static str>,
}

/// 公共 HTTP API 的唯一事实来源。**手工维护**——任何应进入 v1 稳定契约的新
/// 端点都必须在此登记。缺席者按 `internal` 处理，不进 OpenAPI。
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
        // 结构更新与 endpoint 配置刻意分成两条路由：一次改名不该重写那些
        // 根本没变的 JSON 列。
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
        // 服务属于项目，故读与两个写挂在 `/api/projects/:projectId/baseurls`。
        // 只有按 id 的动词留在 `/api/baseurl-defs/:defId`：改名或删除点名的是服务
        // 本身，不需要项目上下文就能找到它。
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
        // 登记它们是因为浏览器端要用：三个动词与 `/api/endpoint-details` 的读
        // 一起构成响应历史条的持久化面。
        //
        // 列表与清除共用一条路径，因为作用对象都是「某个 endpoint 的若干行」；
        // 只有追加是集合级的，它点的 id 在 body 里，见 `routes/response_history.rs`。
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
        // 登记而非留作 internal：`start` / `stop` 是 renderer 侧的 daemon 开关，
        // 与 Tauri 命令 `mcp_daemon_start` / `mcp_daemon_stop` 同源，也是第三方
        // 客户端在运行时控制 daemon 所需的那一对。两者都回与 `/restart` 相同的
        // `{ ok, pid, port }` 信封。
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
        // 以下端点刻意不登记：它们是 supervisor / UI 的投影，无版本承诺。它们
        // 在 `WEB_ROUTES` 里可供运行时访问，但 OpenAPI v1 文档省略。
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
        // `/api/permissions*` 与 `/api/request_settings*` 同样刻意不在公共契约内。
        // baseUrl-def 一族已登记在上方。
    ]
}
