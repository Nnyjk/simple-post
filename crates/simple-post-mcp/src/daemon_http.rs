//! HTTP 模式 MCP daemon —— axum router + JSON-RPC 分发。
//!
//! 手写的 JSON-RPC 2.0 over HTTP（无 SSE、无批处理）。
//!
//! ## 工具级错误为什么走 `result.isError` 而不是 JSON-RPC 的 `error`
//!
//! JSON-RPC 的 `error` 字段只承载**信封级**失败（parse error -32700、
//! invalid request -32600、method not found -32601、invalid params -32602、
//! internal error -32603），它与 `result` 在一个响应里互斥。MCP 则规定
//! `tools/call` 的失败用 `result.isError: true`：工具**跑了**，只是它报告的工作
//! 出错，LLM 仍应读到正文、呈现给用户或自行恢复。
//!
//! 所以本文件把两类失败分流：
//!   * 工具 handler 的失败 → 合成 `result.isError: true`，永不走 `error`
//!   * 信封级失败（JSON 解析不过、`jsonrpc != "2.0"`、method 未知、缺 `params.name`）
//!     → `error`，因为此时还没有工具正文可读
//!
//! 代价：调用方在 JSON-RPC 边界上分不出传输级与工具级失败。真正需要这个区分的地方在
//! 下一层 —— `GET /mcp/healthz` 的 `requests_handled` 与服务端信封失败计数；
//! 不要在响应体里把它重新引回来。

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use simple_post_core::domain::{McpHttpConfig, DEFAULT_MCP_PATH};
use tokio::net::TcpListener;

use crate::mcp::{self, AppState, CallToolResult, JsonRpcError, JsonRpcResponse};

// =========================================================================
// 公共 router 与入口
// =========================================================================

/// 一个挂载前缀下的三条请求路径，依次是 `(json_rpc, healthz, tools)`。
///
/// 刻意与 [`mcp_routes`] 分开且保持纯函数：axum 0.7 的 `Router` 不提供路由清单，
/// 断言「到底挂了哪些路径」的单测必须能不启服务器就问到答案。
pub fn route_paths(path: &str) -> (String, String, String) {
    (
        path.to_string(),
        format!("{path}/healthz"),
        format!("{path}/tools"),
    )
}

/// router 挂载的全部前缀：配置里那个，加上规范路径 `/mcp`（两者不同时）。
///
/// 别名不是装饰：`port_registry::healthz` 探的是写死的
/// `http://127.0.0.1:<port>/mcp/healthz`，改名而不留别名会让 supervisor 报
/// `Unhealthy`、`ensure_started` 直接失败，而 daemon 其实服务得好好的。留着它也
/// 让已配置好、写死 `/mcp` 的 AI 客户端在改名后继续可用。
pub fn mounted_prefixes(config: &McpHttpConfig) -> Vec<String> {
    let path = config.normalized_path();
    if path == DEFAULT_MCP_PATH {
        return vec![path];
    }
    vec![path, DEFAULT_MCP_PATH.to_string()]
}

/// 挂在 `path` 上的三条 AI 客户端路由。
///
/// `path` 必须已归一（以 `/` 开头、不带尾斜杠）—— 走 [`McpHttpConfig::normalized_path`]。
fn mcp_routes(path: &str) -> Router<Arc<AppState>> {
    let (json_rpc, healthz, tools) = route_paths(path);
    Router::new()
        .route(&json_rpc, post(handle_mcp_post))
        .route(&healthz, get(handle_healthz))
        .route(&tools, get(handle_tools))
}

/// 用出厂默认（回环绑定、`/mcp`、关闭远程）构建 MCP HTTP daemon 的 [`Router`]。
///
/// 集成测试要复用与线上完全相同的接线，所以对外暴露；daemon 二进制走的是
/// [`build_router_with_config`]，这个包装只服务默认场景与不关心监听配置的测试。
pub fn build_router(state: Arc<AppState>) -> Router {
    build_router_with_config(state, &McpHttpConfig::default())
}

/// 按用户的 `mcp.http_config` 构建 router。
///
/// 挂载前缀来自 [`mounted_prefixes`]（配置路径 + `/mcp` 别名）；白名单由
/// [`host_origin_guard`] 作为中间件装上，默认只允许回环 —— fail-closed：把
/// 「没配白名单」读成「全都放行」等于让这个开关本身变成漏洞。
pub fn build_router_with_config(state: Arc<AppState>, config: &McpHttpConfig) -> Router {
    mounted_prefixes(config)
        .into_iter()
        .fold(Router::new(), |acc, prefix| acc.merge(mcp_routes(&prefix)))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(config.clone()),
            host_origin_guard,
        ))
        .with_state(state)
}

/// 用默认配置绑定并服务。调用方自己传已绑好的 listener，以便拿到实际端口
/// （`port = 0` 的「OS 分配」技巧）。
pub async fn serve(listener: TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    serve_with_config(listener, state, McpHttpConfig::default()).await
}

/// 在调用方已绑定的 listener 上，按用户的 `mcp.http_config` 服务。
pub async fn serve_with_config(
    listener: TcpListener,
    state: Arc<AppState>,
    config: McpHttpConfig,
) -> std::io::Result<()> {
    let app = build_router_with_config(state, &config);
    axum::serve(listener, app).await
}

// =========================================================================
// Host / Origin 守卫
// =========================================================================
//
// 覆盖整个 MCP router，按以下顺序判定：
//
//   1. `allow_remote == false`（默认）：`Host` 必须解析为回环，或等于配置的绑定 host。
//   2. `allow_remote == true`：非回环 `Host` 必须命中 `allowedHosts`（裸 `*` 通配）。
//      **空列表拒绝一切非回环 host** —— 把「没配白名单」读成「全都放行」，等于让
//      这个开关本身成为漏洞。
//   3. `Origin`：缺省意味着是 AI 客户端（curl、Claude Code、Claude Desktop），放行；
//      存在则必须命中 `allowedOrigins`。
//
// 任何配置下回环都放行，即使用户配了只含内网地址的白名单 ——
// `port_registry::healthz` 正靠这一点继续工作。

/// 剥掉 `Host` 头里的端口。白名单按名字比较而不是 `name:port`，否则用户写
/// `studio.local` 就匹配不上走 `:4173` 进来的客户端。
pub fn host_name(raw: &str) -> String {
    let h = raw.trim();
    if let Some(rest) = h.strip_prefix('[') {
        // 带方括号的 IPv6 字面量，可带可不带 `:port`。裸 IPv6 不是合法的
        // Host 头，所以括号不闭合时原样返回，自然匹配不上任何条目。
        if let Some(close) = rest.find(']') {
            return rest[..close].to_string();
        }
        return h.to_string();
    }
    match h.rsplit_once(':') {
        // 只剥纯数字尾巴：裸 `::1` 有多个冒号，「端口」那段不是数字。
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            name.to_string()
        }
        _ => h.to_string(),
    }
}

/// 单条白名单是否覆盖 `value`。裸 `*` 是通配；主机名大小写不敏感（DNS 本就如此）。
fn entry_matches(pattern: &str, value: &str) -> bool {
    let p = pattern.trim();
    p == "*" || p.eq_ignore_ascii_case(value)
}

/// 守卫的 `Host` 一半。`None` 直接拒：连 `Host` 都没有的请求，不该由 daemon 去猜。
pub fn host_permitted(config: &McpHttpConfig, host_header: Option<&str>) -> bool {
    let Some(raw) = host_header else {
        return false;
    };
    let host = host_name(raw);
    if simple_post_core::domain::is_loopback_host(&host) {
        return true;
    }
    // 配置的绑定 host 永远可接受 —— 否则在具名网卡上开 `allow_remote`，它会拒掉
    // 自己发出的流量。
    if host == host_name(&config.host) {
        return true;
    }
    if !config.allow_remote {
        return false;
    }
    config.allowed_hosts.iter().any(|p| entry_matches(p, &host))
}

/// 守卫的 `Origin` 一半。`None` 放行：AI 客户端发的是裸 HTTP 请求，根本不带
/// `Origin`，强制要求会打断所有真实调用方；这个头只出现在浏览器形状的请求里，
/// 而那正是值得校验的场景。
pub fn origin_permitted(config: &McpHttpConfig, origin: Option<&str>) -> bool {
    let Some(o) = origin else {
        return true;
    };
    config
        .allowed_origins
        .iter()
        .any(|p| p.trim() == "*" || p.trim() == o.trim())
}

/// 合并判定。拆成这样是为了两半都能不起 HTTP 客户端直接单测，中间件只当薄适配层。
pub fn check_access(
    config: &McpHttpConfig,
    host_header: Option<&str>,
    origin: Option<&str>,
) -> Result<(), String> {
    if !host_permitted(config, host_header) {
        return Err(format!(
            "host {:?} is not allowed (allowRemote: {}, allowedHosts: {:?})",
            host_header.unwrap_or("<none>"),
            config.allow_remote,
            config.allowed_hosts
        ));
    }
    if !origin_permitted(config, origin) {
        return Err(format!(
            "origin {:?} is not in allowedOrigins ({:?})",
            origin.unwrap_or("<none>"),
            config.allowed_origins
        ));
    }
    Ok(())
}

/// 403 用与其余后端一致的信封：`{ "error": { "code": "forbidden", "message": "..." } }`，
/// `code` 是稳定契约，`message` 面向人。
fn forbidden_response(message: String) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": {
                "code": "forbidden",
                "message": message,
            }
        })),
    )
        .into_response()
}

async fn host_origin_guard(
    State(config): State<Arc<McpHttpConfig>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    match check_access(&config, host, origin) {
        Ok(()) => next.run(request).await,
        Err(message) => forbidden_response(message),
    }
}

// =========================================================================
// `GET /mcp/healthz`
// =========================================================================

async fn handle_healthz(State(state): State<Arc<AppState>>) -> Json<HealthzResponse> {
    let uptime_ms = state.started_at.elapsed().as_millis() as u64;
    Json(HealthzResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms,
        requests_handled: state
            .metrics
            .requests_handled
            .load(std::sync::atomic::Ordering::Relaxed),
        current_connections: 0, // axum 0.7 没有便宜的取法
        registered_tools: mcp::registered_tool_names(),
    })
}

#[derive(Debug, Serialize)]
pub struct HealthzResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
}

// =========================================================================
// `GET /mcp/tools`
// =========================================================================

async fn handle_tools(State(state): State<Arc<AppState>>) -> Json<ToolsListResponse> {
    Json(ToolsListResponse {
        tools: mcp::tool_descriptors(&state).await,
    })
}

#[derive(Debug, Serialize)]
pub struct ToolsListResponse {
    pub tools: Vec<mcp::ToolDescriptor>,
}

// =========================================================================
// `POST /mcp` — JSON-RPC 2.0 entry point
// =========================================================================

async fn handle_mcp_post(State(state): State<Arc<AppState>>, body: String) -> Response {
    dispatch_mcp_request(state, body).await
}

/// `handle_mcp_post` 的本体。对外暴露，好让 `serve-web` 形状在同一个 `/mcp` 路由
/// 上复用完全相同的分发，而不必再挂一遍 daemon router（那会把 `/mcp` 前缀叠两次）。
pub async fn dispatch_mcp_request(state: Arc<AppState>, body: String) -> Response {
    state
        .metrics
        .requests_handled
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return Json(json_rpc_error(None, -32700, format!("parse error: {e}"))).into_response();
        }
    };

    if request.jsonrpc != "2.0" {
        return Json(json_rpc_error(
            request.id,
            -32600,
            "jsonrpc must be \"2.0\"".to_string(),
        ))
        .into_response();
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id, request.params),
        "ping" => JsonRpcResponse::ok(request.id, json!({})),
        "tools/list" => handle_tools_list(state.clone(), request.id).await,
        "tools/call" => handle_tools_call(state, request.id, request.params).await,
        "notifications/initialized" | "notifications/cancelled" => {
            return StatusCode::NO_CONTENT.into_response();
        }
        other => json_rpc_error(request.id, -32601, format!("method not found: {other}")),
    };

    Json(response).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    id: Option<Value>,
}

fn handle_initialize(id: Option<Value>, _params: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::ok(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "simple-post-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )
}

async fn handle_tools_list(state: Arc<AppState>, id: Option<Value>) -> JsonRpcResponse {
    let tools = mcp::tool_descriptors(&state).await;
    JsonRpcResponse::ok(id, json!({ "tools": tools }))
}

/// 处理 `tools/call`。
///
/// 工具级失败塞进 `result` 的合成 `{ "isError": true }` 正文，不填顶层 `error`
/// —— 按 MCP spec，LLM 必须总能**读到**失败消息才能恢复重试；信封级 `error` 会在
/// LLM 看到正文之前就短路。真正的传输级问题（解析失败、method 未知）仍走
/// `json_rpc_error`。理由全文见本文件开头的章节。
async fn handle_tools_call(
    state: Arc<AppState>,
    id: Option<Value>,
    params: Option<Value>,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return json_rpc_error(id, -32602, "missing params".to_string()),
    };
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n.to_string(),
        None => return json_rpc_error(id, -32602, "params.name (string) required".to_string()),
    };
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    match mcp::dispatch(&state, &name, args).await {
        Ok(result) => JsonRpcResponse::ok_with_result(id, result),
        // 工具 handler 失败 → 塞进 `result.isError`，不走 JSON-RPC 的 `error`。
        // 理由见文件开头的章节。
        //
        // 这里是手写 JSON，**不经过** `CallToolResult`，所以 spec 的 `isError`
        // 拼写也得在这里手打一遍：`CallToolResult` 靠
        // `#[serde(rename_all = "camelCase")]` 拼对，这一支绕开了那个属性。
        Err(e) => JsonRpcResponse::ok(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": format!("error: {e}"),
                }],
                "isError": true,
            }),
        ),
    }
}

fn json_rpc_error(id: Option<Value>, code: i64, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id: id.unwrap_or(Value::Null),
        result: None,
        error: Some(JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}

// =========================================================================
// 小工具：把已序列化的 CallToolResult 渲染成 JSON-RPC
// =========================================================================

/// 只被测试与 `tools/call` 用。之所以不叫 `ok`，是为了收强类型的
/// `CallToolResult`；原来的 `ok` 仍收裸 `Value`，供 `tools/list`、`ping` 等使用。
pub trait JsonRpcResponseExt {
    fn ok_with_result(id: Option<Value>, result: CallToolResult) -> Self;
}

impl JsonRpcResponseExt for JsonRpcResponse {
    fn ok_with_result(id: Option<Value>, result: CallToolResult) -> Self {
        JsonRpcResponse::ok(id, serde_json::to_value(result).unwrap_or(Value::Null))
    }
}

// =========================================================================
// 测试
// =========================================================================
//
// Host / Origin 白名单行为与 router 的路径挂载都放在兄弟文件里，
// 确保它们永远不会去摸 socket。
#[cfg(test)]
#[path = "daemon_http_test.rs"]
mod tests;
