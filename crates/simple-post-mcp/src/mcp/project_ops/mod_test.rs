use super::*;
use crate::mcp::AppState;
use simple_post_core::error::AppError;
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
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(crate::mcp::AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: std::sync::Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
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
async fn module_move_preserves_collections_and_endpoints() {
    // Regression net for a real data-loss bug. `module_move` used to
    // delete-then-recreate the module, and because
    // `collections.module_id` / `endpoints.collection_id` are both
    // `ON DELETE CASCADE`, moving a *populated* module silently
    // destroyed every collection under it and every endpoint under
    // those collections. A real `project_id` UPDATE fixes that.
    use simple_post_core::db::{collections, endpoints, modules, permissions, projects};
    use simple_post_core::domain::{Collection, Endpoint, Module, Project, RequestBody};
    use simple_post_core::permissions::Permission;

    let st = state();

    /// `Project` has no `Default` (every field is meaningful), so
    /// build the two minimal rows the move needs.
    fn mk() -> Project {
        Project {
            id: String::new(),
            name: String::new(),
            description: None,
            base_url: None,
            base_url_definitions: vec![],
            default_base_url_definition_id: None,
            color: String::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(
            c,
            &Project {
                id: "p_move_a".into(),
                name: "A".into(),
                ..mk()
            },
        )?;
        projects::create(
            c,
            &Project {
                id: "p_move_b".into(),
                name: "B".into(),
                ..mk()
            },
        )?;
        modules::create(
            c,
            &Module {
                id: "m_move".into(),
                project_id: "p_move_a".into(),
                name: "M".into(),
                description: None,
                sort_order: 0,
                expanded: None,
            },
        )?;
        collections::create(
            c,
            &Collection {
                id: "c_move".into(),
                module_id: "m_move".into(),
                parent_collection_id: None,
                name: "C".into(),
                description: None,
                sort_order: 0,
                expanded: None,
            },
        )?;
        endpoints::create(
            c,
            &Endpoint {
                id: "e_move".into(),
                collection_id: "c_move".into(),
                name: "E".into(),
                description: None,
                method: simple_post_core::domain::HttpMethod::Get,
                url: "/x".into(),
                environment_id: None,
                base_url_definition_id: None,
                base_url_override: None,
                params: vec![],
                headers: vec![],
                body: RequestBody::default(),
                auth: Default::default(),
                docs: None,
                notes: None,
                tags: vec![],
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
        )?;
        // The per-project gate defaults to `read_call`, which is below
        // the `full` a move requires. Set it after the project row
        // exists — `agent_permissions.project_id` is an FK to projects.
        permissions::set(c, "p_move_b", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    // Move it. The project permission defaults to Full so the
    // per-project gate passes.
    module::tool_module_move(
        &st,
        serde_json::json!({
            "module_id": "m_move",
            "new_project_id": "p_move_b",
            "new_sort_order": 3
        }),
    )
    .await
    .expect("module_move should succeed");

    // The module keeps its id and now belongs to the target project.
    let after = simple_post_core::db::with_conn(&st.pool, |c| modules::get(c, "m_move"))
        .expect("module must still exist under its original id");
    assert_eq!(after.project_id, "p_move_b");
    assert_eq!(after.sort_order, 3);

    // Crucially: the child rows survived the move.
    let c_after = simple_post_core::db::with_conn(&st.pool, |c| collections::get(c, "c_move"))
        .expect("collection must survive the module move");
    assert_eq!(c_after.module_id, "m_move");

    let e_after = simple_post_core::db::with_conn(&st.pool, |c| endpoints::get(c, "e_move"))
        .expect("endpoint must survive the module move");
    assert_eq!(e_after.collection_id, "c_move");
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
    assert_eq!(none.kind, simple_post_core::domain::AuthType::None);

    let bearer = super::endpoint::__test_build_auth(
        Some("bearer"),
        Some(serde_json::json!({"bearer": "tok"})),
    )
    .unwrap();
    assert_eq!(bearer.kind, simple_post_core::domain::AuthType::Bearer);

    let unknown = super::endpoint::__test_build_auth(Some("magic"), None);
    assert!(unknown.is_err());
}
