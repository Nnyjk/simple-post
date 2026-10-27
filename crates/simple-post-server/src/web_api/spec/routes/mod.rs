//! 契约表按资源分组，每组一个文件。
//!
//! 文件名去掉 `.rs` 恰好等于 `table.rs` 里调用的组名：`scripts/dump-openapi.mjs`
//! 按 `public_route_specs()` 里的调用顺序按文件名找 `fn routes`，所以组名与文件名
//! 必须一致，且不能与 `web_api/` 下的别的文件重名——`routes/` 下那批 handler 就叫
//! `projects.rs`、`nodes.rs`、`mcp.rs`，故这一层统一带 `spec_` 前缀。

pub(super) mod spec_baseurls;
pub(super) mod spec_dynamic_variables;
pub(super) mod spec_endpoint_details;
pub(super) mod spec_environments;
pub(super) mod spec_health;
pub(super) mod spec_http;
pub(super) mod spec_interchange;
pub(super) mod spec_mcp;
pub(super) mod spec_nodes;
pub(super) mod spec_oauth2;
pub(super) mod spec_projects;
pub(super) mod spec_response_history;
pub(super) mod spec_upsert;
