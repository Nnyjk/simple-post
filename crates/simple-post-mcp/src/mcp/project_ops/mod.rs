//! Project / module / collection / endpoint / baseUrl-definition MCP tools.
//!
//! Total: 5 + 5 + 5 + 8 + 5 = **28 tools**.
//!
//! ## Permission naming
//!
//! [`simple_post_core::permissions::required_permission`] maps tool names
//! to a `Permission` rank (`Read` < `ReadCall` < `Full`). The map is
//! keyed by the *tool* name, so call sites use e.g. `endpoint_get` and
//! `endpoint_execute` (the latter is `api_call_endpoint` here, since it
//! actually issues a network call).
//!
//! ## Permission gating strategy
//!
//! For tools that take `project_id` directly we pass it to
//! [`simple_post_core::permissions::require`]. For `endpoint_*` tools that don't
//! (get / delete / duplicate / move / search), we look up the endpoint
//! first to find its `project_id`, then gate.
//!
//! ## File layout
//!
//! ```text
//!   project_ops/
//!     mod.rs        ← you are here (dispatch table + shared Args + tests)
//!     project.rs    ← project_* (5 tools)
//!     module.rs     ← module_* (5 tools)
//!     collection.rs ← collection_* (5 tools)
//!     endpoint.rs   ← endpoint_* (8 tools)
//!                  (endpoint_project_id lookup moved to simple_post_core::db::endpoints::project_id_of)
//!     base_url_def.rs ← base_url_def_* (5 tools)
//! ```
//!
//! The `table()` function stitches the per-category `register(&mut v)`
//! functions back together in the original stable order so
//! `tools/list` is unchanged. External callers still do
//! `use crate::mcp::project_ops::*` and reach `table()` through
//! re-exports.

use serde::Deserialize;

use crate::mcp::ToolEntry;

pub(super) mod base_url_def;
pub(super) mod collection;
pub(super) mod endpoint;
pub(super) mod module;
pub(super) mod project;

// =========================================================================
// Shared Args shapes (used by more than one submodule)
// =========================================================================
//
// `ProjectIdArgs` is consumed by `project_get`, `module_list`, and
// `base_url_def_list`. `ModuleIdArgs` is consumed by `collection_list`.
// Each lives once here so the per-entity submodules can `use super::*`
// and stay focused on their own Args.

#[derive(Debug, Deserialize)]
pub(super) struct ProjectIdArgs {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModuleIdArgs {
    pub module_id: String,
}

// =========================================================================
// Dispatch table
// =========================================================================

/// Stitched dispatch table — preserves the original ordering:
/// projects (5) → modules (5) → collections (5) → endpoints (8) →
/// base_url_defs (5). Each submodule's `register()` only pushes its
/// own slice; this function concatenates them so callers see one
/// `table()` with all 28 entries.
pub(crate) fn table() -> Vec<ToolEntry> {
    let mut v = Vec::with_capacity(32);

    // ---- projects (5) ----
    project::register(&mut v);

    // ---- modules (5) ----
    module::register(&mut v);

    // ---- collections (5) ----
    collection::register(&mut v);

    // ---- endpoints (8) ----
    endpoint::register(&mut v);

    // ---- base_url_defs (5) ----
    base_url_def::register(&mut v);

    v
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
