use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::mcp::AppState as McpAppState;
use crate::permissions::CapabilityPolicy;
use simple_post_mcp::daemon_http;

use super::super::dto::McpStatusDto;
use super::super::{map_app_error, HttpErrorResponse, WebState};

pub(crate) async fn mcp_status(State(s): State<Arc<WebState>>) -> Json<McpStatusDto> {
    Json(s.daemon.snapshot().into())
}

/// `GET /api/mcp/logs` —— daemon 最近若干行日志。
///
/// 缓冲是**有界**的，故这份东西永远不长。被复用的（外部启动的）daemon 没有 stderr
/// 管道，那种情况下 `capturing` 为假、`lines` 为空。
pub(crate) async fn mcp_log_tail(
    State(s): State<Arc<WebState>>,
) -> Json<crate::dto::McpLogTailDto> {
    Json(s.daemon.log_tail())
}

// =========================================================================
// MCP HTTP 监听配置
// =========================================================================
//
//   GET /api/mcp/http-config —— 读绑定 host / port / path 与白名单
//   PUT /api/mcp/http-config —— 校验、持久化、报告是否需要重启
//
// 两者都委派给 `commands::mcp_http_config` 的普通函数，故 REST 路由与 Tauri 命令在
// 校验与「是否需要重启」上不会各说各话。
//
// 注意这里配置的是**daemon 子进程**，不是本服务器：`web_api::build_router` 挂载的
// `/mcp` 路由留在规范路径上，而文档推荐的 stdio-bridge → `serve-web` 客户端配置正是
// 依赖这一点。

/// `GET /api/mcp/http-config` —— daemon 的监听配置。`preferences` 行缺失时返回出厂的
/// 仅 loopback 默认值而非 404，好让全新安装渲染出一个表单而不是一个错误。
pub(crate) async fn mcp_http_config_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::domain::McpHttpConfig>, HttpErrorResponse> {
    crate::http_config::read_http_config(&s.pool)
        .map(Json)
        .map_err(map_app_error)
}

/// `PUT` 的 body，接受这条路由合理会收到的两种形状之一。
///
/// * **wrapped** —— `{ "config": { … } }`。这是双 transport 的 `invokeCmd` 发的：
///   它的 `WEB_ROUTES` 条目复刻了 Tauri 命令的参数列表，因为同一个调用点必须对两种
///   transport 都能用。
/// * **bare** —— `{ "host": …, "path": … }`，即文档化的 REST 形状。
///
/// 必须**先试** wrapped：两条变体之所以能共存，靠的就是这个顺序。`McpHttpConfig` 带
/// `#[serde(default)]` 且不拒绝未知字段，所以它会把 `{ "config": { … } }` 当成一个
/// 每个字段都*缺失*的对象愉快接受——于是一次看似成功的保存，实则把用户的 LAN 配置
/// 悄悄覆盖成仅 loopback 默认值。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum McpHttpConfigBody {
    Wrapped {
        config: crate::domain::McpHttpConfig,
    },
    Bare(crate::domain::McpHttpConfig),
}

impl McpHttpConfigBody {
    fn into_config(self) -> crate::domain::McpHttpConfig {
        match self {
            Self::Wrapped { config } | Self::Bare(config) => config,
        }
    }
}

/// `PUT /api/mcp/http-config` —— 整体替换。LAN 绑定配 `allowRemote: false` 会以
/// `validation` 错误拒绝而不是被存下，因为这个组合会造出一个监听在网络上、却什么都
/// 不答的 daemon。
pub(crate) async fn mcp_http_config_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<McpHttpConfigBody>,
) -> Result<Json<crate::http_config::McpHttpConfigDto>, HttpErrorResponse> {
    crate::http_config::write_http_config(&s.pool, body.into_config())
        .map(Json)
        .map_err(map_app_error)
}

// =========================================================================
// MCP 能力策略
// =========================================================================
//
// 镜像 `commands/` 中 Tauri 命令的三个 REST 端点：
//   GET  /api/mcp/capability       —— 读当前策略
//   PUT  /api/mcp/capability       —— 设置策略（body: { policy: "..." }）
//   GET  /api/mcp/capability/tools —— 列出当前策略下可见的工具

#[derive(Debug, Serialize)]
pub(crate) struct CapabilityResponse {
    policy: CapabilityPolicy,
}

/// `GET /api/mcp/capability` —— 读全局能力策略。web 模式的 renderer 每次加载页面都
/// 调它，好让 UI 徽标始终反映 sqlite 里的真值。
pub(crate) async fn mcp_capability_get(
    State(s): State<Arc<WebState>>,
) -> Result<Json<CapabilityResponse>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let policy = match crate::db::with_conn(&pool, crate::db::preferences::mcp_policy) {
        Ok(p) => p,
        Err(e) => return Err(map_app_error(e)),
    };
    Ok(Json(CapabilityResponse { policy }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CapabilitySetBody {
    policy: String,
}

/// `PUT /api/mcp/capability` —— 更新全局策略。经
/// `db::preferences::set_mcp_policy` 持久化，原样回显新策略。
pub(crate) async fn mcp_capability_put(
    State(s): State<Arc<WebState>>,
    Json(body): Json<CapabilitySetBody>,
) -> Result<Json<CapabilityResponse>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let parsed = match CapabilityPolicy::parse(&body.policy) {
        Some(p) => p,
        None => {
            return Err(map_app_error(AppError::Validation(format!(
                "unknown mcp policy '{}' (expected read_only | safe_write | full_access)",
                body.policy
            ))))
        }
    };
    if let Err(e) =
        crate::db::with_conn(&pool, |c| crate::db::preferences::set_mcp_policy(c, parsed))
    {
        return Err(map_app_error(e));
    }
    Ok(Json(CapabilityResponse { policy: parsed }))
}

/// `GET /api/mcp/capability/tools` —— 列出当前策略下可见的工具。复用 JSON-RPC
/// `tools/list` 路径所用的同一个 `mcp::tool_descriptors` 过滤，故输出完全一致。
pub(crate) async fn mcp_capability_list_tools(
    State(s): State<Arc<WebState>>,
) -> Result<Json<Vec<String>>, HttpErrorResponse> {
    let pool = s.pool.clone();
    let state = crate::mcp::AppState {
        pool,
        started_at: std::time::Instant::now(),
        metrics: std::sync::Arc::new(crate::mcp_metrics::McpMetrics::new()),
    };
    Ok(Json(
        crate::mcp::tool_descriptors(&state)
            .await
            .into_iter()
            .map(|t| t.name)
            .collect(),
    ))
}

/// `POST /api/mcp/stop` —— 手动 OFF 开关。
///
/// 响应信封与 `/api/mcp/start`、`/api/mcp/restart` 同为 `RestartResult` 形状：
/// `{ ok, pid, port }`。停止成功回 `{ ok: true, pid: null, port: null }`——之后没有
/// 任何东西在服务，故 `null` 就是答案，不是缺字段。停一个已经停了的 daemon 回
/// `{ ok: true }`，不是错误。
///
/// 终止本进程 spawn 的 daemon，以及仅从端口记录收养来的那个（见
/// `daemon_supervisor::teardown`），然后释放 `mcp.port`。
pub(crate) async fn mcp_stop(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    match s.daemon.stop().await {
        Ok(snapshot) => Ok(Json(crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        })),
        Err(e) => {
            eprintln!("[mcp_stop] failed: {e:?}");
            Ok(Json(crate::dto::RestartResult {
                ok: false,
                pid: None,
                port: None,
            }))
        }
    }
}

/// `POST /api/mcp/start` —— 手动 ON 开关。
///
/// 幂等，而幂等正是重点：对一个健康的 daemon 点「启动」绝不能杀掉重拉——那会丢
/// 掉进行中的 MCP 会话并无谓折腾端口。故它走 supervisor 的幂等 `start`，原样返回既有
/// daemon 的身份，只在什么都没被跟踪时才 spawn。
///
/// 响应信封与 `/api/mcp/stop`、`/api/mcp/restart` 同为 `RestartResult` 形状：
/// `{ ok, pid, port }`，其中 `pid` / `port` 标识当前在服务的 daemon。底层失败
/// （spawn 错误、healthz 超时）按 restart 的做法折叠成
/// `{ ok: false, pid: null, port: null }`。
pub(crate) async fn mcp_start(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    // Tauri 之外没有 `AppHandle`，故这是 supervisor 共享路径的 `serve-web` 绑定：
    // 不从 Tauri 派生任何东西，并取服务器启动时用的数据目录。
    match s.daemon.start_without_app_handle(&s.data_dir).await {
        Ok(snapshot) => Ok(Json(crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        })),
        Err(e) => {
            eprintln!("[mcp_start] failed: {e:?}");
            Ok(Json(crate::dto::RestartResult {
                ok: false,
                pid: None,
                port: None,
            }))
        }
    }
}

// ----- mcp daemon（其余动词）-------------------------------------

/// `POST /api/mcp/restart` —— 杀掉当前 daemon（无论 spawn 还是收养而来）再拉起一个
/// 新的。
///
/// 响应信封 `RestartResult` —— `{ ok, pid, port }` —— 与 `/api/mcp/start` 和
/// `/api/mcp/stop` 逐字节相同。错误被吞成 `{ ok: false, pid: null, port: null }` 而非
/// 5xx，好让三个生命周期动词对 UI 维持统一的 `{ ok: bool }` 契约。
pub(crate) async fn mcp_restart(
    State(s): State<Arc<WebState>>,
) -> Result<Json<crate::dto::RestartResult>, HttpErrorResponse> {
    // 与 Tauri 命令（`mcp_daemon_restart`）相同的吞错信封：错误以
    // `{ ok: false, pid: null, port: null }` 出现而非 5xx，故 web UI 的重启动作保留
    // 统一的 `{ ok: bool }` 契约。
    let result = match s.daemon.restart_without_app_handle(&s.data_dir).await {
        Ok(snapshot) => crate::dto::RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        },
        Err(_) => crate::dto::RestartResult {
            ok: false,
            pid: None,
            port: None,
        },
    };
    Ok(Json(result))
}

pub(crate) async fn mcp_proxy_path() -> String {
    // 内联 `commands::mcp_daemon_proxy_path` 的同一套逻辑，避免依赖 `#[tauri::command]`
    // 宏胶水（那会逼出一个 `tauri::AppHandle` 参数）。
    fn build(_exe_name: &str, proxy_name: &str) -> Option<std::path::PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post.exe", "simple-post-mcp.exe")
    } else {
        build("simple-post", "simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// 把一个 JSON-RPC 请求派发给 MCP dispatcher。包装
/// `daemon_http::dispatch_mcp_request`（daemon 的 `/mcp` handler 核心），使
/// `serve-web` 形态无需 spawn daemon 子进程即可在同一个端口上服务 AI 客户端。
pub(crate) async fn mcp_json_rpc_post(
    state: Arc<McpAppState>,
    body: String,
) -> axum::response::Response {
    daemon_http::dispatch_mcp_request(state, body).await
}

pub(crate) async fn mcp_healthz(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    use daemon_http::HealthzResponse;
    let uptime_ms = state.started_at.elapsed().as_millis() as u64;
    let requests_handled = state
        .metrics
        .requests_handled
        .load(std::sync::atomic::Ordering::Relaxed);
    let registered_tools = crate::mcp::registered_tool_names();
    axum::Json(serde_json::json!(HealthzResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms,
        requests_handled,
        current_connections: 0,
        registered_tools,
    }))
}

pub(crate) async fn mcp_tools(state: Arc<McpAppState>) -> axum::Json<serde_json::Value> {
    let _ = state;
    axum::Json(serde_json::json!({ "tools": crate::mcp::registered_tool_names() }))
}

#[cfg(test)]
#[path = "mcp_test.rs"]
mod tests;
