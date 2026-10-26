// ---------------------------------------------------------------------------
// 创建选项与移动目标 DTO。Patch 类型住在 `crate::domain`，经
// `use crate::domain::*` 引入。
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::domain::*;
use crate::permissions::CapabilityPolicy;
use simple_post_interchange::ir::IrServer;
use simple_post_interchange::Diagnostic;

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

/// 一次发送的结果：传输层的信封 + 脚本的产出。
///
/// **脚本结果可选**：链上没有脚本时它缺席，旧的调用方只读 `http` 那一半即可。
/// 脚本失败**不改变** `http` 里的成败——失败信息在 `script` 里单独呈现。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcomeDto {
    pub http: crate::http_executor::HttpResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptOutcome>,
}

impl From<simple_post_core::script::ScriptedSendOutcome> for SendOutcomeDto {
    fn from(outcome: simple_post_core::script::ScriptedSendOutcome) -> Self {
        Self {
            http: outcome.http,
            script: outcome.script,
        }
    }
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
    /// URL 路径里的占位符与它们各自的值。它们替换进路径本身，不进查询串。
    pub path_params: Option<Vec<KeyValue>>,
    pub docs: Option<String>,
    pub notes: Option<String>,
    /// 请求发出之前跑的那段脚本。缺席 = 没有脚本。
    pub pre_request_script: Option<String>,
    /// 响应回来之后跑的那段脚本。缺席 = 没有脚本。
    pub test_script: Option<String>,
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
            path_params: self.path_params.unwrap_or_default(),
            headers: self.headers.unwrap_or_default(),
            body: self.body.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            docs: self.docs,
            notes: self.notes,
            tags: self.tags.unwrap_or_default(),
            pre_request_script: self.pre_request_script,
            test_script: self.test_script,
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

/// daemon 的日志 tail。设置页用它显示「最近日志」。
///
/// 缓冲是**有界**的（最近 500 行），故这份东西永远不长。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpLogTailDto {
    /// 最近若干行日志。**不含**指标行——那些是机器读的数字，混进来只会碍眼。
    pub lines: Vec<String>,
    /// 本次 daemon 启动的 Unix 毫秒时间戳。重启后它变，故它就是日志的「分界」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    /// 缓冲是否因超出容量而丢过行。用户据此知道「上面还有更早的日志，但已经没了」。
    pub truncated: bool,
    /// 本监管者是否持有这个 daemon 的 stderr 管道。
    ///
    /// 复用一个**外部启动**的 daemon 时没有管道，那种情况下缓冲永远是空的。
    /// 用户看到的应该是「无日志捕获」而不是「没有日志」。
    pub capturing: bool,
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

// ---------------------------------------------------------------------------
// interchange —— 导入 / 导出共用的线形状
// ---------------------------------------------------------------------------

/// 勾选树上的一个分组（导入时是「将要落成集合的那一层」，导出时是集合）。
///
/// **导入与导出共用同一个节点类型**是刻意的：一个组件一份类型，才能真正做到
/// 「共用一个弹窗」，而不是两套形状在组件里各写一半再凑合渲染。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferNode {
    /// 稳定标识。导入时回传给落库接口当选择集，导出时是 `nodes.id`。
    pub key: String,
    pub name: String,
    pub endpoints: Vec<TransferEndpoint>,
    pub children: Vec<TransferNode>,
    /// 本子树里的端点总数（含所有层级）。勾选计数与「全选」都读它。
    pub endpoint_total: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferEndpoint {
    pub key: String,
    pub name: String,
    /// 大写动词。`TRACE` 之类是 `HttpMethod::Custom`，`as_str()` 原样返回。
    pub method: String,
    /// 只有路径。host 不在这里——它住在分组那一层的 `servers` 上。
    pub path: String,
    pub body_mode: BodyMode,
    pub auth: AuthType,
    /// 导入时：该请求与目标项目里已有的某个接口指纹相同。默认**仍然勾上**——
    /// 用户勾的是这份文件而不是某几行，而它在落库时会被跳过（报告里会说）。
    /// 导出时恒为 false。
    pub duplicate: bool,
    /// 落在这一条上的诊断在 `diagnostics` 里的下标。诊断按端点归属，
    /// 否则同一条 pointer 在两个分组下会各显示一次。
    pub diagnostic_indexes: Vec<usize>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCounts {
    pub groups: usize,
    pub endpoints: usize,
    pub duplicates: usize,
}

/// Postman 环境在预览里的一行。它此刻还只是「这个文件里有个叫这个名字的环境」，
/// 真正创建发生在 commit 那一步。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedEnvironmentDto {
    pub name: String,
    pub variable_count: usize,
    /// 源环境里被停用的变量数。它们照样导入并显示，但不参与变量替换。
    pub disabled_variable_count: usize,
}

/// 预览里「这份文档会落成一个服务」的预告。
///
/// **缺席是一个真实答案**，不是「待定」：文档没给 host、或给了多于一个 host 时就是
/// 缺席（后者附一条 `Warn` 诊断说明为什么）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedServiceDto {
    /// 服务名，即 host。项目内唯一。
    pub service: String,
    /// 文档给出的那个 URL。落库时它会被写进目标模块在这个环境下的那一格。
    pub url: String,
    /// 项目里已有同名服务，落库时复用而不是新建。URL 不在服务行上，复用不会改动
    /// 别的模块的地址。
    pub existing: bool,
    /// URL 会写进哪个环境。项目没有激活环境时为 `None`——服务仍然建，只是格子无处可填。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_name: Option<String>,
}

/// 落库报告里已经落下的服务。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedServiceDto {
    pub name: String,
    pub url: String,
    /// 服务定义是新建的（`false` = 复用了项目里已有的同名服务）。
    pub created: bool,
    /// 端点有没有被指向它。为 `false` 只有一种情形：项目没有激活环境。
    pub assigned: bool,
    /// URL 写进了哪个环境。为 `None` 表示没写进去。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    /// `Format::as_str()`，即 `postman-collection-v2.1` / `openapi-3`。
    pub format: String,
    /// 人读的名字。线上名带版本号与连字符，是给程序用的。
    pub format_label: String,
    /// Postman `info.name` / OpenAPI `info.title`。**不落成节点**，只做副标题。
    pub title: Option<String>,
    /// 文件名或 URL 末段。
    pub source_name: String,
    pub groups: Vec<TransferNode>,
    pub counts: TransferCounts,
    pub diagnostics: Vec<Diagnostic>,
    pub environments: Vec<ImportedEnvironmentDto>,
    /// 文档恰好给出一个 host 时有值：会新建（或复用）这个服务并把 URL 填进目标模块。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ProposedServiceDto>,
    /// **仅 URL 模式**回传：拉到的原文。前端原样送回 commit，故一次导入只拉一次
    /// 文档，且 commit 的行为不取决于那之后源站有没有变。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_content: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCommitDto {
    pub created_endpoints: usize,
    pub skipped_duplicates: usize,
    pub created_collection_ids: Vec<String>,
    pub created_endpoint_ids: Vec<String>,
    pub environments_created: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// 实际落下的服务。为 `None` 表示这一次没碰服务表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<AppliedServiceDto>,
}

/// 一个模块在所选环境下的服务 URL。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleServersDto {
    pub module_key: String,
    pub module_name: String,
    /// **空数组是真实答案**：该模块在此环境下没有配 URL，导出时就不写 `servers`，
    /// 不借用别的模块的，也不落回项目 ★。
    pub servers: Vec<IrServer>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreviewDto {
    pub groups: Vec<TransferNode>,
    pub counts: TransferCounts,
    /// 实际用到的环境 id。为 `None` 表示没选出环境（或该项目没有激活环境），
    /// 那时每个模块的 `servers` 都是空的——这是忠实答案，不是「待补」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    pub servers: Vec<ModuleServersDto>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDocumentDto {
    pub file_name: String,
    /// `application/json` / `application/yaml`。渲染层按它选保存对话框的过滤器。
    pub media_type: String,
    pub content: String,
    /// 落文档期间新增的诊断（集合外的动词、手写的 schema 不合法等）。
    pub diagnostics: Vec<Diagnostic>,
}

// ---- 入参 ----

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewOpts {
    pub project_id: String,
    /// 落点：挂进这个已存在的模块。
    pub module_id: String,
    /// 文件内容。与 `url` 二选一；两个都给时 `content` 优先。
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCommitOpts {
    pub project_id: String,
    pub module_id: String,
    /// **始终**是文件原文，不重跑 URL 拉取。预览阶段已经把 URL 模式的原文回传过来，
    /// 故这里再解析一次得到的一定是同一棵树。
    pub content: String,
    /// 勾中的端点 key。空集表示「一个都不导」——那是用户的选择，不是失败。
    #[serde(default)]
    pub selected: Vec<String>,
    /// 是否把文档里的 host 落成服务。**缺席为真**：文档带了 host 就该有服务，
    /// 用户要的是「地址别丢」；关掉它是一种明确的取舍，不是默认值。
    #[serde(default)]
    pub apply_service: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreviewOpts {
    pub project_id: String,
    pub module_id: String,
    /// 按哪个环境解析 `servers`。**缺席或空串 = 取激活环境**（与今天一致）。
    ///
    /// 空串必须与「没传」等价：渲染层「不选环境」那一项送来的就是空串，而空串
    /// 不是一个环境 id——按 id 去查会得到「环境不存在」。
    #[serde(default)]
    pub environment_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDocumentOpts {
    pub project_id: String,
    pub module_id: String,
    /// 空串 = 不选环境。此时 `servers` 恒为空，导出的文档不带 `servers`。
    #[serde(default)]
    pub environment_id: String,
    #[serde(default)]
    pub selected: Vec<String>,
    /// `"json"` / `"yaml"`。认不出的值报 `AppError::Validation`，不静默回退成 JSON。
    pub format: String,
}
