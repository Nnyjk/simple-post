//! `simple-post serve-web` 的 REST + 静态文件层。
//!
//! 局域网浏览器读写与 Tauri 桌面端同一份 SQLite。`/mcp` 的 JSON-RPC
//! 不接守护子进程，直接在本进程跑同一个 `mcp::dispatch`，AI 客户端
//! 无需改动。
//!
//! 布局：`WebState` / `build_router` / `serve` 在这里；handler 按资源拆到
//! `routes/`，wire 类型在 `dto.rs`，`AppError` → HTTP 映射在 `error.rs`，
//! OpenAPI 契约表在 `spec.rs`，SPA 静态处理在 `static_files.rs`。

mod dto;
mod error;
mod routes;
mod spec;
mod static_files;

pub use dto::{ensure_server_id, HealthDto, McpStatusDto};
pub use error::{map_app_error, HttpErrorResponse};
pub use spec::{public_route_specs, RouteSpec};

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    http::Method,
    routing::{get, patch, post},
    Json, Router,
};
use tower_http::services::ServeDir;

use crate::daemon_supervisor::McpDaemonHandle;
use crate::db::{self, DbPool};
use crate::error::AppError;
use crate::mcp::AppState as McpAppState;
use crate::port_registry;

use routes::*;
use static_files::spa_index;

// -------------------------------------------------------------------------
// Shared state.
// -------------------------------------------------------------------------

/// `Clone` 廉价（`DbPool` 是 `Arc<Pool>` 克隆，daemon handle 是
/// `Arc<Mutex<…>>` 克隆）。
#[derive(Clone)]
pub struct WebState {
    pub pool: DbPool,
    pub daemon: McpDaemonHandle,
    pub data_dir: PathBuf,
    /// `/mcp` JSON-RPC 用的 MCP 状态：与 daemon 子进程共用同一 pool 与
    /// metrics，所以指向 `http://server:4173/mcp` 的 AI 客户端与浏览器 SPA
    /// 落在同一份数据上。
    pub mcp_state: Arc<crate::mcp::AppState>,
    /// 可选 bearer 口令。`None` 保持整棵路由树不鉴权；`Some(_)` 则对每个
    /// `/api/*` 与 `/mcp` 请求强制 `auth_middleware`。前端把 bearer 存在
    /// `localStorage`，由 `webFetch()` 带上。
    pub password: Option<String>,
    /// 可选反代 base path（如 `"/sp"`）。设置后整棵 router（含 `/mcp` 与
    /// 三个健康探针）都 `nest` 到该前缀下：`curl /api/projects` 404，
    /// `curl /sp/api/projects` 200。
    pub base_path: Option<String>,
}

// -------------------------------------------------------------------------
// Handlers.
// -------------------------------------------------------------------------

async fn healthz() -> Json<HealthDto> {
    Json(HealthDto {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        shape: "server",
    })
}

// -------------------------------------------------------------------------
// Router.
// -------------------------------------------------------------------------

/// 组装 axum router。`spa_dir` 是 `dist/` 的**绝对**路径，由调用方解析一次传入。
///
/// `state.password` 为 `Some` 时给除 `auth::is_public_path` 公开名单外的
/// 全部路由装上 bearer 中间件。`state.base_path` 为 `Some` 时把整棵 router
/// `nest` 到该前缀下，供反代把 SPA 挂在 `https://host/sp/`。两者默认 `None`。
/// 导入导出四条路由的请求体上限。只给这四条，不给整棵 `api` router——其余动词都是
/// 「按 id 取一行」，抬它们的全局上限只会扩大暴露面。
fn body_limit() -> axum::extract::DefaultBodyLimit {
    axum::extract::DefaultBodyLimit::max(crate::interchange_api::TRANSFER_BODY_LIMIT_BYTES)
}

pub fn build_router(state: WebState, spa_dir: PathBuf) -> Router {
    // 先取出 auth 字段，`Arc::new` 之后结构体就被移走了。
    let password = state.password.clone();
    let base_path = state.base_path.clone();
    let state_arc = Arc::new(state);

    let api = Router::new()
        .route("/api/healthz", get(healthz))
        // 无前缀的探针别名。`auth::is_public_path` 名单里也有它，但
        // **名单成员 ≠ 路由已注册**：漏注册时请求落到 SPA fallback，根路径
        // 回 `200 text/html`、带 base path 时回 404——只 grep `200 OK` 的探针
        // 分不出这两种。健康探针要断言响应体，不能只看状态行。
        //
        // 两条路径共用 `healthz`，不会各自漂移。
        .route("/healthz", get(healthz))
        // ---- projects ----
        .route("/api/projects", get(projects_list).post(projects_create))
        .route(
            "/api/projects/{id}",
            get(projects_get)
                .patch(projects_update)
                .delete(projects_delete),
        )
        .route("/api/http/send", post(http_send))
        // ---- oauth2 ----
        //
        // 取 token 是**发请求之前**的一步，所以它是独立动词而不是 `/api/http/send`
        // 的参数：渲染层先拿到 access token 自己拼好 `Authorization`，再走既有的
        // 发送路径，于是 cURL 预览与真实发送仍是同一个组装核。必须落在
        // `auth::is_public_path` 名单**之外**——它按请求体里的 URL 出一趟站请求。
        .route("/api/oauth2/token", post(oauth2_token))
        // ---- resource tree ----
        //
        // 字面量动词必须注册在 `/{id}` 之前，否则在 radix tree 里被参数段吃掉。
        .route("/api/nodes", get(nodes_list).post(nodes_create))
        .route("/api/nodes/move", post(nodes_move))
        .route(
            "/api/nodes/{id}",
            get(nodes_get).patch(nodes_update).delete(nodes_delete),
        )
        .route(
            "/api/nodes/{id}/endpoint-detail",
            get(node_endpoint_detail_get).patch(node_endpoint_detail_patch),
        )
        // renderer bootstrap 的批量读。独立路径而非 `/api/nodes` 的过滤：它回答的
        // 是「配置」，不是「结构」。
        .route("/api/endpoint-details", get(endpoint_details_list))
        // collection 上的两段脚本。端点的脚本在 endpoint-detail 里（同一行配置），
        // 这一对只管 collection——它住在自己的表里。
        .route(
            "/api/nodes/{id}/scripts",
            get(node_scripts_get).patch(node_scripts_patch),
        )
        .route("/api/nodes/{id}/duplicate", post(node_duplicate))
        // ---- environments ----
        .route(
            "/api/environments",
            get(environments_list).post(environments_create),
        )
        // 同样的 radix tree 优先级：`/active`（字面量）注册在 `/{envId}`（参数）之前。
        .route("/api/environments/active", post(environments_set_active))
        .route(
            "/api/environments/{envId}",
            patch(environments_update).delete(environments_delete),
        )
        .route("/api/environments/{envId}/vars", post(environments_set_var))
        .route(
            "/api/environments/{envId}/vars/disabled",
            post(environments_set_var_disabled),
        )
        .route(
            "/api/environments/{envId}/vars/rename",
            post(environments_rename_var),
        )
        .route(
            "/api/environments/{envId}/module-baseurls",
            post(environments_set_module_baseurl),
        )
        // ---- baseUrlDefs ----
        // 服务属于项目，故三个 project 级动词挂在 `/api/projects/{projectId}/baseurls`。
        // `/baseurls/default` 与 `/api/baseurl-defs/{defId}` 前缀不同，不存在 radix
        // tree 优先级问题；`base_url_defs_test` 盯住每一条都仍可达。
        .route(
            "/api/projects/{projectId}/baseurls",
            get(base_url_defs_list).post(base_url_defs_create),
        )
        .route(
            "/api/projects/{projectId}/baseurls/default",
            post(base_url_defs_set_default),
        )
        .route(
            "/api/baseurl-defs/{defId}",
            patch(base_url_defs_update).delete(base_url_defs_delete),
        )
        // ---- request settings (singleton) ----
        .route(
            "/api/request-settings",
            get(request_settings_get).put(request_settings_put),
        )
        // ---- permissions (per project) ----
        .route(
            "/api/permissions/{projectId}",
            get(permissions_get).put(permissions_set),
        )
        // ---- response history ----
        //
        // 每个动词都指向单个 endpoint。`/api/response-history` 与
        // `/api/response-history/{endpointId}` 必须在 axum 的 radix tree 里共存：
        // 抢优先级失败的那条会落到 SPA fallback，回 `200` + HTML，curl 看着就像
        // 一个正常工作的端点。`response_history_test.rs` 用真实 listener 盯这一条。
        .route("/api/response-history", post(response_history_append))
        .route(
            "/api/response-history/{endpointId}",
            get(response_history_list).delete(response_history_clear),
        )
        // ---- interchange（导入 / 导出）----
        //
        // **必须落在 `auth::is_public_path` 名单之外**，理由与 `/api/oauth2/token`
        // 同族：commit 写库，且 import/preview 的 URL 模式会按用户给的地址出站请求。
        // 浏览器形态下这些是唯一一条能读任意用户指定 URL 的通道。
        //
        // 全部 POST：请求体装得下整份文档（一份 3 MB 的 OpenAPI yaml），而这四个
        // 动词里没有一个是「按 id 取一行」。
        //
        // 故四条都要显式抬 `DefaultBodyLimit`：axum 的默认值是 2 MB，而它们的请求体
        // 装的是整份文档。少了这一层，超大文档在浏览器形态下回的是 413（传输层错误，
        // 读不出「文档太大」），桌面形态却能正常导入——**两侧行为分叉且无一声报错**。
        // 上限值与 `FETCH_MAX_KB` 同源，见 `interchange_api::TRANSFER_BODY_LIMIT_BYTES`。
        .route(
            "/api/interchange/import/preview",
            post(interchange_import_preview).layer(body_limit()),
        )
        .route(
            "/api/interchange/import/commit",
            post(interchange_import_commit).layer(body_limit()),
        )
        .route(
            "/api/interchange/export/preview",
            post(interchange_export_preview).layer(body_limit()),
        )
        .route(
            "/api/interchange/export/document",
            post(interchange_export_document).layer(body_limit()),
        )
        // ---- mcp (existing + new verbs) ----
        .route("/api/mcp/status", get(mcp_status))
        .route("/api/mcp/logs", get(mcp_log_tail))
        .route("/api/mcp/start", post(mcp_start))
        .route("/api/mcp/stop", post(mcp_stop))
        .route("/api/mcp/restart", post(mcp_restart))
        .route("/api/mcp/proxy-path", get(mcp_proxy_path))
        // ---- mcp capability policy ----
        .route(
            "/api/mcp/capability",
            get(mcp_capability_get).put(mcp_capability_put),
        )
        .route("/api/mcp/capability/tools", get(mcp_capability_list_tools))
        // ---- mcp http listener config (daemon bind/path/allowlists) ----
        .route(
            "/api/mcp/http-config",
            get(mcp_http_config_get).put(mcp_http_config_put),
        )
        // ---- preferences ----
        .route(
            "/api/preferences",
            get(preferences_get).put(preferences_put),
        )
        .with_state(state_arc.clone());

    // `/mcp` JSON-RPC + `/mcp/healthz` + `/mcp/tools`。不复用
    // `daemon_http::build_router`：它的内层路由自带 `/mcp` 前缀，再挂一次就成了
    // `/mcp/mcp`。这里直接对共享的 `mcp_state` 注册——与 daemon 子进程同一
    // pool + metrics。
    let mcp_app_state = state_arc.mcp_state.clone();
    let mcp_app_state_2 = mcp_app_state.clone();
    let mcp_app_state_3 = mcp_app_state.clone();
    let mcp_routes = Router::new()
        .route(
            "/mcp",
            post(move |body: String| mcp_json_rpc_post(mcp_app_state, body)),
        )
        .route("/mcp/healthz", get(move || mcp_healthz(mcp_app_state_2)))
        .route("/mcp/tools", get(move || mcp_tools(mcp_app_state_3)));

    // 静态 SPA。`ServeDir` 给 `/assets/*` 正确的 Range / ETag / Last-Modified，
    // fallback 交给 `index.html` 以支持前端路由。
    //
    // 根是 `spa_dir` 本身而非 `spa_dir/assets`：请求路径已带 `/assets/` 前缀，
    // 指到 `spa_dir/assets` 会去找 `dist/assets/assets/…` 而 miss，落进
    // `index.html` fallback，于是每个 JS/CSS 都回 `200 text/html`。状态码看不出
    // 问题，要看 body 的 content-type。
    //
    // fallback 是带 base 的 shell 而非裸 `ServeFile`：深链 `/sp/some/route`
    // 必须带浏览器能解析的 asset URL，否则在该 URL 上刷新即白屏。
    let serve_dir = ServeDir::new(spa_dir.clone()).fallback(axum::routing::any({
        let spa_fallback = spa_dir.clone();
        let base_fallback = base_path.clone();
        move || {
            let spa = spa_fallback.clone();
            let base = base_fallback.clone();
            async move { spa_index(spa, base).await }
        }
    }));

    // 在鉴权层消费掉 `state` 之前取 `base_path`（该层会拿走 `state.password`
    // 的克隆）。
    let auth_state = crate::auth::AuthState::new(password.as_deref());

    // 只给这一棵 router 装鉴权层；shell 与静态树挂在外面。浏览器无法给
    // `<script type="module">` 或样式表请求附 `Authorization: Bearer`，
    // 把 `/assets/*` 拦在层内会让每个 JS/CSS 都 401，页面全白。
    let guarded = {
        let spa_root = spa_dir.clone();
        let base_root = base_path.clone();
        Router::new()
            .merge(api)
            .merge(mcp_routes)
            .route(
                "/",
                get(move || spa_index(spa_root.clone(), base_root.clone())),
            )
            .layer(axum::middleware::from_fn_with_state(
                auth_state,
                crate::auth::auth_middleware,
            ))
    };

    // 其余（`/assets/*`、`/favicon.svg`、以及回落 shell 的前端路由）一律公开，
    // 与 `/` 同等待遇。fallback 只能挂在外层 router：merge 链里只允许一个。
    let inner = Router::new().merge(guarded).fallback_service(serve_dir);

    // 可选地把整棵 router（含 SPA fallback）nest 到 `base_path` 下，供
    // nginx / Caddy 等把应用挂在 `https://server/sp/`。前缀之外不再重复发布：
    // shell 自带前缀的 asset URL 并把前缀交给 renderer，见
    // `static_files::shell_with_base`。未设置时按根路径服务。
    match base_path.as_deref() {
        Some(p) if !p.is_empty() => {
            eprintln!("[serve-web] base_path: {p}");
            // axum 的 `nest` 只匹配裸前缀 `/sp`，`/sp/` 不会转发给内层的 `/`（会 404）。
            // 反代示例常把 `/sp` 重定向到 `/sp/`，所以尾斜杠形式同样返回 shell。
            // 刻意不加鉴权层：这就是 `/sp` 已在发的同一份公开静态内容（见
            // `auth::is_public_path` 名单里的 `/`），其下的 API 仍受保护。
            let prefix = p.trim_end_matches('/');
            let spa_shell = spa_dir.clone();
            let base_shell = base_path.clone();
            Router::new().nest(prefix, inner).route(
                &format!("{prefix}/"),
                get(move || spa_index(spa_shell.clone(), base_shell.clone())),
            )
        }
        _ => inner,
    }
}

// -------------------------------------------------------------------------
// Entry point used by `lib.rs::run_serve_web`.
// -------------------------------------------------------------------------

/// 绑定 TCP listener 并服务到 SIGINT/SIGTERM。
///
/// `data_dir` 供 daemon supervisor 定位 SQLite 文件，必须与桌面端
/// `dirs::data_dir().join("com.simple-post")` 的约定一致，两种形态才共用同一
/// 数据库。
///
/// `auto_start_daemon` 为 `Some(bool)` 时 CLI 已给定答案，`None` 时由持久化的
/// `mcp.auto_start` 行决定。在这里解析而非在 `main.rs`：那边会另开一个
/// `db::init()` 池只为读一个 preference，冷启动就付两次 schema 检查与两个连接池。
pub async fn serve(
    bind_addr: std::net::SocketAddr,
    data_dir: PathBuf,
    spa_dir: PathBuf,
    auto_start_daemon: Option<bool>,
    password: Option<String>,
    base_path: Option<String>,
) -> Result<(), AppError> {
    let pool = db::init(&data_dir)?;
    // 守护进程在 spawn 时从这个池读 `mcp.http_config`，使子进程命令行与设置面板一致。
    let daemon = McpDaemonHandle::with_pool(pool.clone());

    // 在两个值被移进 `WebState` 之前定好守护进程决策：省掉第二次 `db::init()`，
    // 且后台任务预热的 handle 就是 `/api/mcp/*` 读写的那一个。
    let want_daemon = match auto_start_daemon {
        Some(v) => v,
        None => db::with_conn(&pool, db::preferences::mcp_auto_start).unwrap_or(false),
    };

    // `/mcp` JSON-RPC 与本进程共用 `mcp_state`：pool 与 GUI 的 `simple-post.db`
    // 同源，metrics 按进程新建故每次重启归零。
    let mcp_state = Arc::new(McpAppState {
        pool: pool.clone(),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    });

    let state = WebState {
        pool,
        daemon: daemon.clone(),
        data_dir: data_dir.clone(),
        mcp_state,
        password,
        base_path,
    };
    let router = build_router(state, spa_dir);

    eprintln!("[serve-web] listening on http://{bind_addr}");
    eprintln!("[serve-web] data dir: {}", data_dir.display());

    // 必须先 bind 再预热 MCP daemon。`restart_without_app_handle` 是固定 200ms
    // sleep + Windows 上两次 `tasklist` + 拉起本可执行文件的 `mcp` 模式子进程 +
    // 一次 healthz；排在 bind 之前，浏览器全程对着 connection refused，表现为
    // 白屏。
    //
    // 先 bind 则它变成真正的后台任务：daemon 与页面并行起来，
    // `/api/mcp/status` 在就绪前报 `Starting`——GUI 已在暴露这个瞬态。
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Validation(format!("bind {bind_addr}: {e}")))?;

    if want_daemon {
        let dir = data_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = daemon.restart_without_app_handle(&dir).await {
                eprintln!("[serve-web] ensure_started failed: {e:?}");
            }
        });
    } else {
        eprintln!(
            "[serve-web] mcp.auto_start is off — daemon left stopped; \
             use POST /api/mcp/start or the settings panel to enable."
        );
    }

    axum::serve(listener, router)
        .await
        .map_err(|e| AppError::Validation(format!("axum::serve: {e}")))
}

/// 从当前可执行文件向上找 `dist/`。`simple-post.exe` 位于
/// `<project_root>/src-tauri/target/…`，向上若干级即到项目根；找不到时回落
/// `<cwd>/dist`。
pub fn default_spa_dir() -> PathBuf {
    // 最多向上找 6 级，够 `target/debug/` 与 `target/release/` 两种布局。
    let mut here = std::env::current_exe().ok();
    for _ in 0..6 {
        let Some(p) = here.as_ref() else { break };
        let candidate = p.parent().map(|d| d.join("dist"));
        if let Some(c) = candidate {
            if c.is_dir() {
                return c;
            }
        }
        here = p.parent().map(|x| x.to_path_buf());
    }
    // 回落 `cwd/dist`（在项目根执行 `cargo run` 时命中）。
    std::env::current_dir()
        .map(|d| d.join("dist"))
        .unwrap_or_else(|_| PathBuf::from("dist"))
}

/// 数据目录。与 `port_registry::data_dir()` 同一解析结果——守护子进程与本进程
/// 必须落在同一处，`mcp.port` 才发现得到。
pub fn default_data_dir() -> Result<PathBuf, AppError> {
    port_registry::data_dir()
}

// 占位：`Method` 目前未用于按方法分路由，保留以备扩展。
#[allow(dead_code)]
const _UNUSED_METHOD: Method = Method::GET;
