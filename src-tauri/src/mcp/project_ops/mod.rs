//! Project / module / collection / endpoint / baseUrl-definition MCP tools.
//!
//! Total: 5 + 5 + 5 + 8 + 5 = **28 tools**.
//!
//! ## Permission naming
//!
//! [`crate::permissions::required_permission`] maps tool names
//! to a `Permission` rank (`Read` < `ReadCall` < `Full`). The map is
//! keyed by the *tool* name, so call sites use e.g. `endpoint_get` and
//! `endpoint_execute` (the latter is `api_call_endpoint` here, since it
//! actually issues a network call).
//!
//! ## Permission gating strategy
//!
//! For tools that take `project_id` directly we pass it to
//! [`crate::permissions::require`]. For `endpoint_*` tools that don't
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
//!                  (endpoint_project_id lookup moved to crate::db::endpoints::project_id_of)
//!     base_url_def.rs ← base_url_def_* (5 tools)
//! ```
//!
//! The `table()` function stitches the per-category `register(&mut v)`
//! functions back together in the original stable order so
//! `tools/list` is unchanged. External callers still do
//! `use crate::mcp::project_ops::*` and reach `table()` through
//! re-exports.

use serde::Deserialize;

use crate::mcp::{AppState, ToolEntry};

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
pub fn table() -> Vec<ToolEntry> {
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
mod tests {
    use super::*;
    use crate::error::AppError;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    /// Per-call counter that disambiguates temp dirs when several tests
    /// construct their state() in the same millisecond (cargo's default
    /// runs tests in parallel).
    static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn state() -> std::sync::Arc<AppState> {
        let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "simple-post-mcp-project-ops-test-{}-{}-{}",
            std::process::id(),
            crate::domain::now_ms(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::sync::Arc::new(crate::mcp::AppState {
            pool: crate::db::init(&dir).unwrap(),
            started_at: Instant::now(),
            metrics: std::sync::Arc::new(crate::mcp_metrics::McpMetrics::new()),
        })
    }

    #[test]
    fn table_count_is_28() {
        assert_eq!(table().len(), 28);
    }

    #[tokio::test]
    async fn project_create_rejects_empty_name() {
        let res = project::tool_project_create(&state(), serde_json::json!({ "name": "  " })).await;
        assert!(matches!(res, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn endpoint_create_rejects_empty_url() {
        let res = endpoint::tool_endpoint_create(
            &state(),
            serde_json::json!({
                "collection_id": "c1",
                "name": "n",
                "method": "GET",
                "url": ""
            }),
        )
        .await;
        assert!(matches!(res, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn endpoint_create_rejects_unknown_method_gracefully() {
        // Empty method is malformed HTTP — surface a Validation error
        // (the LLM / UI gets `is_error=true` text) instead of panicking
        // or persisting garbage. Custom token-shape verbs like
        // "PROPFIND" / "FROBNICATE" still pass through as
        // `HttpMethod::Custom`.
        let res = endpoint::tool_endpoint_create(
            &state(),
            serde_json::json!({
                "collection_id": "c1",
                "name": "n",
                "method": "",
                "url": "/x"
            }),
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "expected Validation rejection, got {res:?}"
        );
    }

    /// `parse_body_mode` lives in `endpoint.rs` (only used by
    /// `endpoint_create`). Test it through the `endpoint` submodule's
    /// `pub(super)` visibility — actually, it's file-private; re-expose
    /// it via a tiny test-only wrapper.
    #[test]
    fn parse_body_mode_known_values() {
        // Reach the helper through `super::endpoint::__test_parse_body_mode`
        // (defined below as a test-only re-export) so we don't have to
        // make `parse_body_mode` `pub(super)` for production.
        assert!(super::endpoint::__test_parse_body_mode("json").is_ok());
        assert!(super::endpoint::__test_parse_body_mode("form").is_ok());
        assert!(super::endpoint::__test_parse_body_mode("raw").is_ok());
        assert!(super::endpoint::__test_parse_body_mode("none").is_ok());
        assert!(super::endpoint::__test_parse_body_mode("wat").is_err());
    }

    #[test]
    fn build_auth_handles_each_kind() {
        let none = super::endpoint::__test_build_auth(Some("none"), None).unwrap();
        assert_eq!(none.kind, crate::domain::AuthType::None);

        let bearer = super::endpoint::__test_build_auth(Some("bearer"), Some(serde_json::json!({"bearer": "tok"}))).unwrap();
        assert_eq!(bearer.kind, crate::domain::AuthType::Bearer);

        let unknown = super::endpoint::__test_build_auth(Some("magic"), None);
        assert!(unknown.is_err());
    }
}