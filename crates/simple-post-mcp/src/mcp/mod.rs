//! MCP daemon —— 顶层入口、dispatch 表与共享类型。
//!
//! ```text
//!   mcp/
//!     mod.rs             ← 当前文件（入口 + dispatch + 共享类型）
//!     env_ops.rs         ← 8 个 env_* 工具
//!     project_ops/       ← project / module / collection / endpoint / baseUrlDef
//!     http_ops.rs        ← api_run_request, api_call_endpoint
//!     variable_fill.rs   ← api_resolve_template / endpoint / validate
//!     response_parse.rs  ← api_parse_response / diff / extract_jsonpath
//! ```
//!
//! 本模块只做胶水，不定义任何域概念：域类型、DB、HTTP 执行、变量解析、响应解析、
//! 权限全在 `simple_post_core::domain` / `::db` / `::http_executor` / `::permissions`
//! 与 `crate::variable_fill` / `crate::response_parse` 里，这里只有 dispatch 接线
//! 和 JSON-RPC 信封类型。

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod env_ops;
pub mod http_ops;
pub mod project_ops;
pub mod response_parse;
pub mod variable_fill;

use simple_post_core::permissions::{resolve_capability, Capability, CapabilityPolicy};

// =========================================================================
// 公共入口：serve_http
// =========================================================================

/// 在 `bind:port` 上起 MCP HTTP daemon，返回**实际**端口。
///
/// `port = 0` 表示交给 OS 分配（测试与写端口文件的 dev supervisor 需要）。
/// `lib.rs::run_mcp` 的唯一入口。
pub async fn serve_http(
    pool: simple_post_core::db::DbPool,
    port: u16,
    bind: &str,
) -> Result<u16, simple_post_core::error::AppError> {
    let listener = tokio::net::TcpListener::bind((bind, port))
        .await
        .map_err(|e| simple_post_core::error::AppError::Http(format!("bind {bind}:{port}: {e}")))?;
    let actual_port = listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| simple_post_core::error::AppError::Http(format!("local_addr: {e}")))?;

    let state = Arc::new(AppState {
        pool,
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    });

    tokio::spawn(async move {
        if let Err(e) = crate::daemon_http::serve(listener, state).await {
            tracing::error!(error = %e, "MCP daemon exited with error");
        }
    });

    Ok(actual_port)
}

// =========================================================================
// 全 daemon 共享状态
// =========================================================================

/// 每个 axum handler 共享的 daemon 状态。
pub struct AppState {
    /// DB 连接池。`r2d2::Pool` clone 廉价（内部是 Arc），`AppState` 外面还包了一层
    /// Arc，router 每个 handler 拿的是那一份的 clone；真正访问时再 clone 一次池。
    pub pool: simple_post_core::db::DbPool,
    /// uptime 报告的墙钟锚点。
    pub started_at: Instant,
    /// 实时计数器，每处理一个请求就自增，供 `/mcp/healthz` 与 `mcp_status` 展示。
    pub metrics: Arc<simple_post_core::mcp_metrics::McpMetrics>,
}

// =========================================================================
// JSON-RPC 信封（daemon_http.rs 会再导出）
// =========================================================================

/// JSON-RPC 2.0 响应信封。`result` / `error` 恰有一个是 `Some`，另一个为
/// `None`，被 `#[serde(skip_serializing_if)]` 从线上抹掉。
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 成功响应。
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: Some(result),
            error: None,
        }
    }

    /// 传输层错误响应（不是工具层错误）。
    pub fn err(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// =========================================================================
// tools/call 的返回值
// =========================================================================

/// `tools/call` 放在 `result` 里的形状，对应 MCP 的 `CallToolResult`
/// （子集：目前只发 text content）。
///
/// `rename_all = "camelCase"` 是硬约束，不是风格：spec 里失败标志叫 `isError`，
/// 原样序列化 Rust 字段名会被合规客户端整个忽略，于是 LLM 把失败的调用读成成功。
/// 本结构是 `tools/call` result 的唯一塑形来源，所以属性就是全部修复手段。
/// 下面 `CallToolContent` 的 `rename_all = "snake_case"` 与此无关 —— 它管的是
/// `type` **标签**，而该标签只有 `text` 一个变体。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<CallToolContent>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_error: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallToolContent {
    Text { text: String },
}

impl CallToolResult {
    /// 成功结果，包一层 JSON 值（序列化成 text）。
    pub fn json(value: impl Serialize) -> Self {
        let text = serde_json::to_string(&value)
            .unwrap_or_else(|e| format!("{{\"__serialize_error\":\"{}\"}}", e));
        Self {
            content: vec![CallToolContent::Text { text }],
            is_error: false,
        }
    }

    /// 成功结果，包一段纯文本。
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// 错误结果。按 spec，`is_error: true` 仍算一次**成功**的工具调用，
    /// 这样 LLM 能读到消息并继续对话。
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: true,
        }
    }
}

// =========================================================================
// 工具描述符
// =========================================================================

/// 已注册工具的静态描述 —— 名称、说明、`arguments` 的 JSON Schema，`tools/list` 返回。
///
/// `rename_all = "camelCase"` 是硬约束：spec 把这个字段写作 `inputSchema`，发出
/// `input_schema` 会让每个合规客户端都认定该工具没有参数，模型只能从描述文字里
/// 猜参数列表。本 crate 从不回读 descriptor（`Deserialize` 未使用），所以只有
/// 断言序列化结果才拦得住 —— 见 `mod_test.rs`。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

// =========================================================================
// 工具函数签名
// =========================================================================

/// 所有工具都走这个签名。`args` 是客户端发来的完整 `arguments`（没有则为
/// `Value::Null`），缺失/类型不对的字段由 handler 自己归一成
/// `AppError::Validation`。
///
/// `'a` 这个 HRTB 把返回 future 的生命周期绑到 `&AppState` 的借用上；去掉它，
/// boxed `dyn Future` 会退回 `'static`，`&AppState` 借用就过不了编译。
pub type ToolFn = for<'a> fn(
    state: &'a AppState,
    args: Value,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<CallToolResult, simple_post_core::error::AppError>>
            + Send
            + 'a,
    >,
>;

/// 把 `async fn tool_xxx(&AppState, Value) -> Result<...>` 包成 dispatch 表
/// 里存的 boxed-future `ToolFn`。每个 `func:` 槽位都用它：
///
/// ```ignore
/// ToolEntry { name: ..., func: tool_fn!(tool_xxx), ... }
/// ```
///
/// `#[macro_export]` 把它放在 crate 根，调用处只写 `crate::tool_fn!(...)`。
/// 宏体里的路径除 `serde_json::Value` 外都带 `$crate::` 前缀，调用方无需任何
/// `use`；`serde_json::Value` 不带前缀是因为每个子模块都已经 import 了它。
#[macro_export]
macro_rules! tool_fn {
    ($f:path) => {{
        fn __boxed<'a>(
            state: &'a $crate::mcp::AppState,
            args: serde_json::Value,
        ) -> ::std::pin::Pin<
            Box<
                dyn ::std::future::Future<
                        Output = ::std::result::Result<
                            $crate::mcp::CallToolResult,
                            ::simple_post_core::error::AppError,
                        >,
                    > + ::std::marker::Send
                    + 'a,
            >,
        > {
            Box::pin($f(state, args))
        }
        __boxed
    }};
}

// =========================================================================
// dispatch 表
// =========================================================================

/// dispatch 表的一行。手写而不用 `HashMap`，为了让查表无分支且 `tools/list`
/// 的顺序稳定。
pub(crate) struct ToolEntry {
    name: &'static str,
    description: &'static str,
    /// `arguments` 对象的 JSON Schema。存成 `Value` 字面量，`cargo check`
    /// 不需要 `schemars` 往返。
    schema: Value,
    func: ToolFn,
}

/// 完整工具注册表。这里的顺序就是 `tools/list` 的顺序，按类别分组，方便人在
/// `/mcp/tools` 里看。
fn tool_table() -> &'static [ToolEntry] {
    static TABLE: std::sync::OnceLock<Vec<ToolEntry>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = Vec::with_capacity(64);

        // env_ops：8 个
        v.extend(env_ops::table());

        // project_ops：project + module + collection + endpoint + baseUrl
        v.extend(project_ops::table());

        // http_ops：2 个
        v.extend(http_ops::table());

        // variable_fill（MCP 适配层）：3 个
        v.extend(variable_fill::table());

        // response_parse（MCP 适配层）：3 个
        v.extend(response_parse::table());

        v
    })
}

/// 全部工具名的快照（给 `/mcp/healthz` 与自检）。
///
/// **注意**这是未过滤的全量注册表，不是用户在当前策略下看得见的那份；要
/// 「按策略之后」的那份请用 [`tool_descriptors`]。healthz 与 metrics 都
/// 不需要用户的视角。
pub fn registered_tool_names() -> Vec<String> {
    tool_table().iter().map(|e| e.name.to_string()).collect()
}

/// 工具名 → 它需要的全局 [`Capability`]。
///
/// **穷尽**：`tool_table()` 里每一条都必须在此有行。两道能力门在缺行时都
/// *fail closed*，所以少一行的工具是被拒绝，而不是被默认放行；
/// `every_registered_tool_has_a_capability` 是让两张表同步的回归网。
///
/// 用户全局 `read_only` / `safe_write` / `full_access` 的执行点在这里，
/// 因此本表不能有默认值。按项目的
/// [`simple_post_core::permissions::required_permission`] 是另一道更粗的门
/// （未知工具默认 `Permission::Full`），两者不要互相顶替。
pub const REQUIRED_CAPABILITY: &[(&str, Capability)] = &[
    // env
    ("env_list", Capability::Read),
    ("env_get_active", Capability::Read),
    ("env_create", Capability::Create),
    ("env_update", Capability::Update),
    ("env_delete", Capability::Delete),
    ("env_set_var", Capability::Update),
    ("env_set_baseurl", Capability::Update),
    ("env_set_active", Capability::Update),
    // project
    ("project_list", Capability::Read),
    ("project_get", Capability::Read),
    ("project_create", Capability::Create),
    ("project_update", Capability::Update),
    ("project_delete", Capability::Delete),
    // module
    ("module_list", Capability::Read),
    ("module_create", Capability::Create),
    ("module_update", Capability::Update),
    ("module_delete", Capability::Delete),
    ("module_move", Capability::Update),
    // collection
    ("collection_list", Capability::Read),
    ("collection_create", Capability::Create),
    ("collection_update", Capability::Update),
    ("collection_delete", Capability::Delete),
    ("collection_move", Capability::Update),
    // endpoint
    ("endpoint_list", Capability::Read),
    ("endpoint_get", Capability::Read),
    ("endpoint_search", Capability::Read),
    ("endpoint_create", Capability::Create),
    ("endpoint_update", Capability::Update),
    ("endpoint_delete", Capability::Delete),
    ("endpoint_duplicate", Capability::Create),
    ("endpoint_move", Capability::Update),
    // baseUrl definitions
    ("base_url_def_list", Capability::Read),
    ("base_url_def_create", Capability::Create),
    ("base_url_def_update", Capability::Update),
    ("base_url_def_delete", Capability::Delete),
    ("base_url_def_set_default", Capability::Update),
    // HTTP 执行 —— 唯二会发外网请求的工具，信任级别最高。
    ("api_run_request", Capability::Send),
    ("api_call_endpoint", Capability::Send),
    // 读侧的变量 / 端点辅助
    ("api_resolve_template", Capability::Read),
    ("api_resolve_endpoint", Capability::Read),
    ("api_validate_endpoint", Capability::Read),
    ("api_parse_response", Capability::Read),
    ("api_diff_responses", Capability::Read),
    ("api_extract_jsonpath", Capability::Read),
];

/// 在小表 `REQUIRED_CAPABILITY` 上线性查。未知名返回 `None`，调用方必须自己
/// 走「拒绝」分支。表只有「每工具一行」这么大，哈希表属于过早优化。
pub fn required_capability(name: &str) -> Option<Capability> {
    REQUIRED_CAPABILITY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// 全部工具描述符的快照（给 `tools/list` 与 `/mcp/tools`）。按当前
/// [`CapabilityPolicy`] 滤掉策略不覆盖的工具 —— 这是 LLM 可见清单的唯一真相，
/// 配错的策略绝不能把写工具泄给 AI。
///
/// 未设置策略时取 [`CapabilityPolicy::ReadOnly`]（safe-by-default）：没进过
/// 设置面板的用户只看到只读清单。
pub async fn tool_descriptors(state: &AppState) -> Vec<ToolDescriptor> {
    let policy = resolve_capability(&state.pool, None)
        .await
        .unwrap_or(CapabilityPolicy::default());
    tool_table()
        .iter()
        .filter(|e| {
            // fail **closed**：注册了却没有 capability 行的工具是注册表 bug，
            // 藏起来比把用户没批准过的写工具暴露给 AI 客户端安全。
            required_capability(e.name)
                .map(|req| policy.satisfies(req))
                .unwrap_or(false)
        })
        .map(|e| ToolDescriptor {
            name: e.name.to_string(),
            description: e.description.to_string(),
            input_schema: e.schema.clone(),
        })
        .collect()
}

/// 按名字 dispatch 一次 `tools/call`。未知名字 → `AppError::NotFound`
/// （JSON-RPC 层折成 `is_error: true`，LLM 能看到缺的是哪个工具）。
///
/// 进 handler 前再过一次全局能力门（`tools/list` 那道在 `tool_descriptors`）。
/// 策略为 `ReadOnly` 时直接 `tools/call` 一个写工具，拿到的是
/// `AppError::PermissionDenied`，handler 根本不会执行。
pub async fn dispatch(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<CallToolResult, simple_post_core::error::AppError> {
    // 先解析 handler：未注册的名字必须回 `NotFound`，而不是被能力门掩盖成
    // `PermissionDenied`。
    let entry = tool_table()
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| {
            simple_post_core::error::AppError::NotFound(format!("tool '{name}' not registered"))
        })?;

    // 能力预检：策略取法与 `tool_descriptors` 完全一致（此处无项目维度，只有
    // 全局偏好），这样不复查 `tools/list` 就直接 `tools/call` 的客户端同样
    // 无法改动任何东西。缺行 = 拒绝：缺行永远是注册表 bug，不是隐式放行。
    match required_capability(name) {
        Some(req) => {
            let policy = resolve_capability(&state.pool, None).await?;
            if !policy.satisfies(req) {
                return Err(simple_post_core::error::AppError::PermissionDenied(
                    format!(
                        "tool '{name}' requires {:?}, current policy is {:?}",
                        req, policy
                    ),
                ));
            }
        }
        None => {
            return Err(simple_post_core::error::AppError::PermissionDenied(
                format!(
                    "tool '{name}' has no capability classification; \
                     refusing to execute (registry bug — add it to REQUIRED_CAPABILITY)"
                ),
            ));
        }
    }
    (entry.func)(state, args).await
}

// =========================================================================
// 参数辅助函数（每个工具实现都会用）
// =========================================================================

/// 取必填字符串字段。缺失或类型不对 → error。
pub fn arg_str<'a>(
    args: &'a Value,
    key: &str,
) -> Result<&'a str, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-string '{key}'"))
    })
}

/// 取可选字符串字段。缺失或 null 都返回 `None`。
pub fn arg_str_opt(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(String::from)
}

/// 取必填 u64 字段。
pub fn arg_u64(args: &Value, key: &str) -> Result<u64, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-u64 '{key}'"))
    })
}

/// 取可选 u64 字段。
pub fn arg_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

/// 取必填布尔字段。
pub fn arg_bool(args: &Value, key: &str) -> Result<bool, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_bool).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-bool '{key}'"))
    })
}

/// 取可选布尔字段，缺失时为 `false`。
pub fn arg_bool_opt(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

// =========================================================================
// 权限辅助函数 —— simple_post_core::permissions 的薄封装，
// 免得每个调用处自己 import。
// =========================================================================

/// 查项目存下来的权限档位，与 `tool_name` 需要的档位比较；不通过则返回
/// `AppError::PermissionDenied`。调用处**必须**用 `?` 传播 —— 吞掉它就等于
/// 门形同虚设。
pub async fn check_permission(
    pool: &simple_post_core::db::DbPool,
    project_id: &str,
    tool_name: &str,
) -> Result<(), simple_post_core::error::AppError> {
    let required = simple_post_core::permissions::required_permission(tool_name);
    simple_post_core::db::with_conn(pool, |conn| {
        simple_post_core::permissions::require(conn, project_id, required)
    })
}

// =========================================================================
// 测试
// =========================================================================

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
