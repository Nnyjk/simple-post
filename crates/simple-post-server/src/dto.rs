// ---------------------------------------------------------------------------
// 创建选项与移动目标 DTO。Patch 类型住在 `crate::domain`，经
// `use crate::domain::*` 引入。
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::domain::*;
use crate::permissions::CapabilityPolicy;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectOpts {
    pub description: Option<String>,
    pub color: Option<String>,
    // 没有 `baseUrl`。base URL 属于（环境 × 模块 × 服务）三者的交叉格，而新建
    // 项目时用户还没起服务名；那一格矩阵由用户在设置里填。
}

/// Tauri 与 Web 共用的能力策略返回形状。
#[derive(Debug, Clone, Serialize)]
pub struct McpCapabilityDto {
    pub policy: CapabilityPolicy,
}

/// `nodes_create` 随结构节点一并携带的 endpoint 请求配置。
///
/// 整块可选、字段全可选：从树上的「新建请求」建出来的 endpoint 起始为空、事后
/// 再填，每一列在这种情形下都有合理默认值。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointDetailOpts {
    pub method: Option<HttpMethod>,
    pub url: Option<String>,
    pub environment_id: Option<String>,
    pub base_url_definition_id: Option<String>,
    pub body: Option<RequestBody>,
    pub auth: Option<AuthConfig>,
    pub tags: Option<Vec<String>>,
    pub headers: Option<Vec<KeyValue>>,
    pub params: Option<Vec<KeyValue>>,
    pub docs: Option<String>,
    pub notes: Option<String>,
}

impl CreateEndpointDetailOpts {
    /// 展开成 `db::nodes::create` 要写的那一行。`node_id` 由这里传入而非留给调用方，
    /// 免得 detail 指向一个从未插入的节点。
    ///
    /// 之所以是 `pub` 而非 `pub(crate)`：`POST /api/nodes`（本 crate）与
    /// `nodes_create` Tauri 命令（`src-tauri`）接受同一个 opts 类型并都经这里展开，
    /// 两条 transport 才能对半填的 endpoint 采用同样的默认值。
    pub fn into_detail(self, node_id: &str) -> EndpointDetail {
        EndpointDetail {
            node_id: node_id.to_string(),
            method: self.method.unwrap_or_default(),
            url: self.url.unwrap_or_default(),
            environment_id: self.environment_id,
            base_url_definition_id: self.base_url_definition_id,
            base_url_override: None,
            params: self.params.unwrap_or_default(),
            headers: self.headers.unwrap_or_default(),
            body: self.body.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            docs: self.docs,
            notes: self.notes,
            tags: self.tags.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnvironmentOpts {
    pub base_urls: Option<std::collections::HashMap<String, String>>,
    pub variables: Option<std::collections::HashMap<String, String>>,
    pub make_active: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBaseUrlDefOpts {
    pub description: Option<String>,
    /// 客户端自铸的 id，非空白时照单采纳。
    ///
    /// renderer 在这一行存在之前就已经在两个地方持有这个 id——项目的 ★，以及它顺带
    /// 种下的每个（环境 × 模块）URL 单元格——故服务端铸出的 id 会让两者都指向孤儿。
    /// 见 [`adopt_or_mint_id`]，两条 transport 都调它。
    pub id: Option<String>,
}

/// 采纳调用方给的 id，没有则铸一个。
///
/// 两条 transport（Tauri 命令的 `opts.id` 与 REST body 的 `id`）共用这一条规则，
/// 「服务 id 归谁铸」就不会在两边漂移：由 renderer 铸，是因为它在行存在之前就需要这个
/// id。空白按缺失处理，免得 `""`（或纯空白）造出空 id 的行。
pub fn adopt_or_mint_id(requested: Option<String>) -> String {
    match requested {
        Some(id) if !id.trim().is_empty() => id,
        _ => crate::domain::uid(),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettingsPatch {
    pub timeout_ms: Option<u64>,
    pub follow_redirects: Option<bool>,
    pub max_response_size_kb: Option<u64>,
}

// ---------------------------------------------------------------------------
// MCP daemon DTO —— daemon 线格式的唯一事实来源。
// `crate::daemon_supervisor::McpDaemonHandle::snapshot()` 返回它，
// `mcp_status` Tauri 命令原样转发。
// ---------------------------------------------------------------------------

/// 内嵌 MCP daemon 的快照。由 `mcp_status` 返回，UI 周期性轮询以展示「AI 桥接是否
/// 在跑」。
///
/// 线格式与 `src/lib/mcp-types.ts::McpUiState` 完全一致。`status` 是生命周期真值
/// （经 `as_str()` 镜像内部的 `DaemonStatus` 枚举）；`running` 保留为派生布尔量，
/// 让仍消费旧形状的调用方无需强制迁移。
///
/// 这些类型都不含 Tauri 专有内容——只是 `Serialize` / `Deserialize` 的袋子——故放在
/// 两个 host 之下；`src-tauri/src/commands/dto.rs` 再导出本模块，旧的
/// `crate::commands::Foo` 路径仍解析到同一批类型。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    /// 生命周期状态，取值 `"stopped" | "starting" | "running" |
    /// "restarting" | "crashed" | "unhealthy"`。只读 `running` 的消费者不受影响，
    /// 因为我们置 `running = (status == "running")`。
    pub status: String,
    /// 向后兼容布尔量，当且仅当 `status == "running"` 为真。旧调用方可继续读它而不必
    /// 认识生命周期枚举；徽标 / UI 逻辑已改用 `status`。
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    /// 当前活跃的 MCP 客户端连接数。metrics 模块尚未暴露对应计数器，故 supervisor
    /// 在 daemon 通过 stderr 上报之前一律写 `0`；UI 把它显示成一行「0 connections」。
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl McpStatusDto {
    /// 安全的「未运行」快照。`registered_tools` 为空是因为没有 daemon 就没有工具可
    /// 暴露——调用方会在 daemon 回来后的下一次轮询拿到工具列表。
    pub fn stopped() -> Self {
        Self {
            status: "stopped".to_string(),
            running: false,
            pid: None,
            port: None,
            uptime_ms: 0,
            requests_handled: 0,
            current_connections: 0,
            registered_tools: Vec::new(),
            last_error: None,
        }
    }
}

/// `mcp_daemon_restart` Tauri 命令的返回值。与 `McpStatusDto` 分开，是因为重启路径
/// 是唯一一处刻意吞掉底层 `AppError` 的地方——好让 UI 渲染统一的
/// `{ ok: false, ... }` 信封，而不必在每个调用点再挂一层 `.catch`。
///
/// 失败路径上 `pid` / `port` 刻意序列化成 JSON `null`（而非跳过），使响应形状稳定：
/// `{ ok, pid, port }` 恒在。这与 renderer 的 `RestartResult` 类型（见
/// `src/lib/mcp-types.ts`）对齐。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResult {
    pub ok: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}
