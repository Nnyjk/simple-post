//! 公共 HTTP API 契约表。
//!
//! `RouteSpec` 是「哪些 `/api/*` 路由属于稳定契约」的唯一事实来源。
//! `scripts/dump-openapi.mjs` 读这份表（配合 `src/lib/tauri-commands.ts` 的
//! `WEB_ROUTES`）生成 OpenAPI 文档。不在 `public_route_specs()` 里的一律按
//! `internal` 处理——不在 v1 稳定契约内，无向后兼容承诺。
//!
//! 稳定性分级：
//!   * `public`   —— 端点形状、鉴权、响应码稳定。破坏性变更需走 `/api/v2/*` 迁移。
//!   * `internal` —— 无版本承诺，只供 UI / supervisor 使用。
//!
//! 本表为手工维护，**故意**不枚举每一条 axum 路由绑定（如 `/mcp/*` 属于另一套
//! JSON-RPC MCP 契约，另行记录）。
//!
//! 布局：`RouteSpec` 结构体在这里，`public_route_specs()` 在 `table.rs`，
//! 每个资源分组一个文件放在 `routes/` 下。

mod routes;
mod table;

pub use table::public_route_specs;

use serde::Serialize;

/// 一条稳定契约里的路由。
#[derive(Debug, Clone, Serialize)]
pub struct RouteSpec {
    /// HTTP 方法（GET / POST / PUT / PATCH / DELETE）。
    pub method: &'static str,
    /// 路径模板（axum 语法：`/api/projects/{id}`，同时是合法的 OpenAPI 模板）。
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

// ---- internal (not in OpenAPI v1) -------------------------------
// 以下端点刻意不登记：它们是 supervisor / UI 的投影，无版本承诺。它们
// 在 `WEB_ROUTES` 里可供运行时访问，但 OpenAPI v1 文档省略。
//   GET  /api/mcp/status             - daemon status (UI)
//   GET  /api/mcp/logs               - daemon log tail (UI)
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
// baseUrl-def 一族已登记在 `spec_baseurls` 组。
