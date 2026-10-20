//! Simple Post 的 Tauri 2 后端。
//!
//! - `http_request` 让 React 经 `reqwest` 发真实请求而非浏览器 `fetch`：
//!   调用从 Rust 侧出去，webview 的 CORS 不适用。
//! - 契约对齐 `src/lib/http.ts::sendRequest`，前端换传输不必改调用点。
//! - 把 `commands/` 的 `#[tauri::command]` 接到 managed state，并以子进程
//!   拉起内嵌 MCP daemon，供 UI 展示实时状态。
//!
//! ## 无头的一半
//! `web_api`、`daemon_supervisor`、`auth`、`port_registry`、`dto`、
//! `http_config` 与 `mcp` / `serve-web` 入口）在 `simple-post-server`，
//! 下方整体 `pub use` 回来，`commands/` 里的 `crate::…` 路径因此照旧可解析。
//!
//! 原因：`tauri` 是本 crate 的无条件依赖，Linux 容器跑 `serve-web` 就得装上整套
//! GTK/WebKit。依赖方向定为「本 crate → server crate」，镜像才能无 Tauri 构建。

use tauri::Manager;

pub mod commands;

pub use simple_post_core::{db, domain, error, http_executor, mcp_metrics, oauth2, permissions};
pub use simple_post_mcp::{daemon_http, mcp};

// 无头宿主整体转发：用 `pub use` 而非逐个包装函数，`commands/` 才不必因拆 crate 而改。
// `interchange_api` 在其中：导入 / 导出的逻辑住在 server crate，好让 Tauri 命令与
// REST 路由调同一个函数。
pub use simple_post_server::{
    auth, daemon_supervisor, dto, http_config, interchange_api, port_registry, web_api,
};

// CLI 入口。`serve-web` / `mcp` 与宿主无关，归 server crate；桌面二进制仍转发到它们，
// `cargo run -- serve-web`（即 `npm run dev:split`）行为不变。
pub use simple_post_server::{init_tracing, run_mcp, run_mcp_proxy, run_serve_web};

// ----- HTTP request command ------------------------------------------------
//
// 实际 I/O 在 `crate::http_executor::execute`；这里只是转发，让 `generate_handler!`
// 能在 `lib.rs` 里找到 `http_request`。

/// 经 `reqwest` 发真实请求，绕过浏览器 CORS。Tauri 外壳内走此路，纯 Vite 下前端退回
/// `fetch`，改 UI 无需重编 Rust。
///
/// 必须保持非 `pub`：`#[tauri::command]` 与 `generate_handler!` 都会生成内部的
/// `__cmd__http_request` 宏，`pub` 时在同一 crate 根文件撞名（E0255）。
/// 见 https://v2.tauri.app/develop/calling-rust/ 。
#[tauri::command]
async fn http_request(
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout_ms: u64,
    follow_redirects: bool,
    max_response_size_kb: u64,
) -> crate::http_executor::HttpResult {
    let input = crate::http_executor::HttpRequestInput {
        method,
        url,
        headers,
        body,
        timeout_ms,
        follow_redirects,
        max_response_size_kb,
    };
    crate::http_executor::execute(&input).await
}

// ----- Tauri entry point ----------------------------------------------------

/// Tauri GUI 入口：把 `McpDaemonHandle` 与 DB pool 放进 managed state（供
/// `State<…>` 读取）、注册全部命令、在 `.setup()` 里异步拉起 daemon 子进程。
///
/// supervisor 走 `tokio::spawn` 而非就地 await，窗口才能先渲染，5 秒 healthz
/// 等待也不占主 run-loop。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    use crate::daemon_supervisor::McpDaemonHandle;

    // 先于 webview 建好 GUI 自己的 DB pool，`State<DbPool>` 的命令首次 invoke 即可解析。
    // data_dir 约定与 daemon 相同，SQLite WAL 让两个进程共享同一文件。
    let data_dir = match crate::port_registry::data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[gui] cannot resolve data dir: {e:?}");
            std::process::exit(1);
        }
    };
    let pool = match crate::db::init(&data_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[gui] db::init({}) failed: {e:?}", data_dir.display());
            std::process::exit(1);
        }
    };

    let daemon_handle = McpDaemonHandle::with_pool(pool.clone());

    tauri::Builder::default()
        // 导出的「另存为」。`serve-web` 形态没有 Tauri，前端在浏览器里退回 Blob 下载，
        // 故这两个插件只在桌面壳注册，且前端侧是动态 import——见
        // `src/lib/transfer-file-save.ts`。
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(pool)
        .manage(daemon_handle)
        .invoke_handler(tauri::generate_handler![
            // 本地发请求的命令。
            http_request,
            // 刻意没有「按 endpoint 发送」的命令：前端组装完整请求后交给 `http_request`。
            crate::commands::projects_list,
            crate::commands::projects_get,
            crate::commands::projects_create,
            crate::commands::projects_update,
            crate::commands::projects_delete,
            crate::commands::nodes_list,
            crate::commands::nodes_get,
            crate::commands::nodes_create,
            crate::commands::nodes_update,
            crate::commands::nodes_delete,
            crate::commands::nodes_move,
            crate::commands::nodes_duplicate,
            crate::commands::endpoint_detail_get,
            crate::commands::endpoint_details_list,
            crate::commands::endpoint_detail_update,
            crate::commands::environments_list,
            crate::commands::environments_create,
            crate::commands::environments_update,
            crate::commands::environments_delete,
            crate::commands::environments_set_var,
            crate::commands::environments_set_var_disabled,
            crate::commands::environments_rename_var,
            crate::commands::environments_set_module_baseurl,
            crate::commands::environments_set_active,
            crate::commands::base_url_defs_list,
            crate::commands::base_url_defs_create,
            crate::commands::base_url_defs_update,
            crate::commands::base_url_defs_delete,
            crate::commands::base_url_defs_set_default,
            crate::commands::request_settings_get,
            crate::commands::request_settings_update,
            // 响应历史：按 endpoint 的 FIFO，上限在 db 写入侧截断。
            crate::commands::response_history_list,
            crate::commands::response_history_append,
            crate::commands::response_history_clear,
            // OAuth2 取 token。取 token 是发请求**之前**的一步，故与 `http_request`
            // 是两个命令而不是它的参数：web 形态下前端据此先把 Authorization 组装好。
            crate::commands::oauth2_token,
            crate::commands::permissions_get,
            crate::commands::permissions_set,
            crate::commands::mcp_status,
            crate::commands::mcp_daemon_start,
            crate::commands::mcp_daemon_stop,
            crate::commands::mcp_daemon_restart,
            crate::commands::mcp_daemon_proxy_path,
            crate::commands::mcp_auto_start_get,
            crate::commands::mcp_auto_start_set,
            crate::commands::mcp_capability_get,
            crate::commands::mcp_capability_set,
            crate::commands::mcp_capability_list_tools,
            crate::commands::mcp_http_config_get,
            crate::commands::mcp_http_config_set,
            // 导入 / 导出（Postman v2.1 ↔ OpenAPI 3.0）。四个命令全是薄壳，REST 侧同名
            // 路由调的是同一批函数。
            crate::commands::interchange_import_preview,
            crate::commands::interchange_import_commit,
            crate::commands::interchange_export_preview,
            crate::commands::interchange_export_document,
        ])
        .setup(|app| {
            // 先读持久化的 mcp.auto_start（默认 false，需用户在设置里开启）再拉 supervisor。
            let pool: tauri::State<crate::db::DbPool> = app.state();
            let auto_start = match crate::db::with_conn(pool.inner(), |c| {
                crate::db::preferences::mcp_auto_start(c)
            }) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[gui] failed to read mcp.auto_start, defaulting to off: {e:?}");
                    false
                }
            };

            // state() 只借用，clone（Arc 支撑，便宜）后再 move 进 spawn。
            let handle = app.state::<McpDaemonHandle>().inner().clone();
            let app_handle = app.handle().clone();
            // 必须用 `tauri::async_runtime::spawn`，不是 `tokio::spawn`：`.setup()`
            // 在 tokio runtime 之外同步执行，裸 `tokio::spawn` 会 panic
            // "there is no reactor running"。
            tauri::async_runtime::spawn(async move {
                if !auto_start {
                    eprintln!(
                        "[gui] mcp.auto_start is off — leaving daemon stopped; \
                         toggle it in Settings → Connection to start."
                    );
                    return;
                }
                if let Err(e) = handle.ensure_started(&app_handle).await {
                    // 不因拉起失败而 panic GUI：supervisor 已置 Crashed，渲染侧经 `mcp_status`
                    // （或 `daemon://crashed` toast）呈现。
                    eprintln!("[daemon] ensure_started failed: {e:?}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
