//! 公共 HTTP API 契约表。
//!
//! `RouteSpec` 是「哪些 `/api/*` 路由属于稳定契约」的唯一事实来源。
//! `scripts/dump-openapi.mjs` 读这份表（配合 `src/lib/tauri-commands.ts` 的
//! `WEB_ROUTES`）生成 OpenAPI 文档。不在 `public_route_specs()` 里的一律按
//! `internal` 处理——不在 v1 稳定契约内，无向后兼容承诺。
//!
//! 稳定性分级：
//!   * `public`   —— 端点形状、鉴权、响应码稳定。破坏性变更需走 `/api/v2/*` 迁移。
//!   * `internal` —— 无版本承诺，只供 UI / supervisor 使用。internal 端点不进本表，
//!     故 `RouteSpec::stability` 目前只取 `"public"`。
//!
//! 本表为手工维护，**故意**不枚举每一条 axum 路由绑定。MCP 的 JSON-RPC 端点不在
//! 主端口上，它由守护进程自己那个监听端口提供，属另一套契约。
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
    /// 稳定性标记。表里的每一行都是 `"public"`；标为 internal 的端点不进本表，
    /// 所以这个字段目前只有一个取值。OpenAPI dumper 把它写成 `x-stability`。
    pub stability: &'static str,
    /// 可选的请求体形状名（由 OpenAPI dumper 查表）。`None` 表示无请求体，
    /// 或该形状尚未记录。
    pub request: Option<&'static str>,
    /// 可选的响应形状名。`None` 会让 dumper 输出 `unknown` 占位。
    pub response: Option<&'static str>,
}

// ---- internal (not in OpenAPI v1) -------------------------------
// 以下端点在 `build_router` 里注册了，但刻意不登记：它们是设置面板与 supervisor
// 的投影，无版本承诺。它们在 `WEB_ROUTES` 里可供运行时访问，但 OpenAPI v1 文档
// 省略它们。
//   GET  /api/nodes/children           - 懒加载层级树：取一个节点的直接孩子
//   GET  /api/nodes/search             - 跨全部项目搜接口并带回它所在的路径
//   GET  /api/request-settings         - 出站请求旋钮的单例行
//   PUT  /api/request-settings
//   GET  /api/permissions/{projectId}   - 每个项目的权限档位
//   PUT  /api/permissions/{projectId}
//   PUT  /api/response-history/favorite - 标记或取消标记一条历史
//   GET  /api/response-history/keep     - 读全局保留条数
//   PUT  /api/response-history/keep
//   GET  /api/mcp/status               - daemon 生命周期快照
//   GET  /api/mcp/logs                 - daemon 日志 tail
//   POST /api/mcp/restart              - 重启 daemon
//   GET  /api/mcp/proxy-path           - `simple-post-mcp` 二进制的路径
//   GET  /api/mcp/capability           - 能力策略
//   PUT  /api/mcp/capability
//   GET  /api/mcp/capability/tools     - 当前策略下可见的工具名
//   GET  /api/mcp/http-config          - daemon 的绑定地址、挂载路径与白名单
//   PUT  /api/mcp/http-config
//   GET  /api/preferences              - 偏好表的原始读写
//   PUT  /api/preferences
//   GET  /healthz                      - 无前缀的健康探针别名（与 `/api/healthz` 共用同一个处理函数）
//
// MCP 的 JSON-RPC 端点、它的健康探针与工具清单都在**守护进程自己的监听端口**上，
// 不在主端口上，故不在本表范围内。
//
// 地址那一族只有一个动词：`POST /api/environments/{envId}/module-baseurls`，登记在
// `spec_environments` 组。
