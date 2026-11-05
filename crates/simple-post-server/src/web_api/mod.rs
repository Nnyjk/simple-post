//! `simple-post serve-web` 的 REST + 静态文件层。
//!
//! 局域网浏览器读写与 Tauri 桌面端同一份 SQLite。
//!
//! 这一层**不提供 MCP**：MCP 的端点是守护进程自己的监听端口（见
//! `daemon_supervisor`），主端口只有 REST 与静态文件。
//!
//! 布局：路由树（`WebState` / `build_router`）在 `router.rs`，启动入口
//! （`serve`）在 `serve.rs`，handler 按资源拆到 `routes/`，wire 类型在
//! `dto.rs`，`AppError` → HTTP 映射在 `error.rs`，OpenAPI 契约表在 `spec/`，
//! SPA 静态处理在 `static_files.rs`。

mod dto;
mod error;
mod router;
mod routes;
mod serve;
mod spec;
mod static_files;

pub use dto::{ensure_server_id, HealthDto};
pub use error::{map_app_error, HttpErrorResponse};
pub use router::{build_router, WebState};
pub use serve::{default_data_dir, default_spa_dir, serve};
pub use spec::{public_route_specs, RouteSpec};

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
