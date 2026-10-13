//! All Tauri commands exposed to the renderer.
//!
//! Each `#[tauri::command]` is a thin wrapper around the corresponding
//! `crate::db::...` data-access function; the `send` module additionally
//! holds the shared endpoint-send entry point used by both transports.
//! Argument shapes are kept aligned with the TS contract in
//! `src/types/domain.ts` and the call sites in `src/lib/tauri*.ts`.
//! Renames on the wire follow Tauri 2 defaults (`camelCase`).
//!
//! Authority split:
//!   * Mutations write through `crate::db::with_tx` so partial state
//!     never escapes.
//!   * Reads go through `crate::db::with_conn` (read-only transaction).
//!   * The `mcp_status` command is the one exception — it pulls from
//!     `crate::daemon_supervisor::McpDaemonHandle`, not the DB. The DTO
//!     shape used by both sides is declared here: see [`McpStatusDto`].
//!
//! Patch types come from `crate::domain` (the canonical place for
//! shared shapes). Create-options and move-targets that don't fit the
//! domain crate live in [`dto`] as small adapter DTOs.
//!
//! Layout mirrors `web_api/routes/`: one module per resource, the
//! shared wire shapes in `dto`, and `send` holding the one place a
//! configured endpoint becomes an outbound HTTP request. The modules
//! themselves are private — everything below is re-exported here, so
//! `crate::commands::<name>` keeps resolving for `generate_handler!`
//! in `lib.rs` and for the REST handlers in `web_api/`.
//!
//! Note on `uid` and the default project color: they live in
//! `crate::domain` (re-exported from `simple-post-core::domain`). The
//! `db` crate also re-uses them, so keeping a single definition avoids
//! drift between REST, Tauri command, and SQLite test paths.

mod base_url_defs;
// `pub(crate)` because the REST route deserialises the same
// `CreateProjectOpts` the Tauri command takes — the two transports have
// to agree on what a create carries, and two copies of the DTO would be
// exactly the drift this file's header warns about.
pub(crate) mod dto;
mod environments;
mod mcp_auto_start;
mod mcp_capability;
mod mcp_http_config;
mod mcp_status;
mod nodes;
mod permissions;
mod projects;
mod request_settings;

// The globs (not item lists) are load-bearing: `#[tauri::command]` also emits
// two `#[macro_export]`ed helper macros next to the function —
// `__cmd__<name>` and `__tauri_command_name_<name>` — and
// `generate_handler!` rewrites `crate::commands::<name>` into a call to
// `crate::commands::__cmd__<name>`, i.e. it resolves the macro *through the
// path's module*, not through the crate-root alias `#[macro_export]` also
// creates. A glob carries those macros up to `commands`; an item list would
// leave them behind in the submodule and E0433 would follow.
pub use base_url_defs::*;
pub use dto::*;
pub use environments::*;
pub use mcp_auto_start::*;
pub use mcp_capability::*;
pub use mcp_http_config::*;
pub use mcp_status::*;
pub use nodes::*;
pub use permissions::*;
pub use projects::*;
pub use request_settings::*;
