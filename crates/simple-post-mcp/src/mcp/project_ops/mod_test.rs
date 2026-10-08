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
    use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
    use simple_post_core::domain::{
        EndpointDetail, Project, RequestBody, ResourceNode, ResourceNodeType,
    };
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

    fn node(root: &str, id: &str, kind: ResourceNodeType, parent: Option<&str>) -> ResourceNode {
        ResourceNode {
            id: id.into(),
            root_id: root.into(),
            parent_id: parent.map(String::from),
            kind,
            name: "n".into(),
            description: None,
            sort_order: 0,
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
        nodes::create(
            c,
            &ResourceNode {
                sort_order: 0,
                ..node("p_move_a", "m_move", ResourceNodeType::Module, None)
            },
            None,
        )?;
        // A sibling left behind in the source project, so the test can
        // see the gap close up behind the module that left.
        nodes::create(
            c,
            &ResourceNode {
                sort_order: 1,
                ..node("p_move_a", "m_stays", ResourceNodeType::Module, None)
            },
            None,
        )?;
        // Two modules already in the destination, so the requested index
        // is meaningful rather than clamped into an empty list.
        for (id, order) in [("m_b0", 0), ("m_b1", 1)] {
            nodes::create(
                c,
                &ResourceNode {
                    sort_order: order,
                    ..node("p_move_b", id, ResourceNodeType::Module, None)
                },
                None,
            )?;
        }
        nodes::create(
            c,
            &node(
                "p_move_a",
                "c_move",
                ResourceNodeType::Collection,
                Some("m_move"),
            ),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_move_a",
                "e_move",
                ResourceNodeType::Endpoint,
                Some("c_move"),
            ),
            Some(&EndpointDetail {
                node_id: "e_move".into(),
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
            }),
        )?;
        // The per-project gate defaults to `read_call`, which is below
        // the `full` a move requires. Set it after the project row
        // exists — `agent_permissions.project_id` is an FK to projects.
        permissions::set(c, "p_move_b", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    // Move it to the middle of the destination project's modules.
    module::tool_module_move(
        &st,
        serde_json::json!({
            "module_id": "m_move",
            "new_project_id": "p_move_b",
            "new_sort_order": 1
        }),
    )
    .await
    .expect("module_move should succeed");

    // The module keeps its id and now belongs to the target project.
    let after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "m_move"))
        .expect("module must still exist under its original id");
    assert_eq!(after.root_id, "p_move_b");
    assert_eq!(after.sort_order, 1);

    // Both projects' top-level orderings stay contiguous: the
    // destination's modules close around the insertion, and the
    // project being left has no gap where the module used to be.
    let top_level = |root: &str| {
        simple_post_core::db::with_conn(&st.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(c, root, ResourceNodeType::Module)
        })
        .unwrap()
        .into_iter()
        .map(|n| (n.id, n.sort_order))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        top_level("p_move_b"),
        vec![("m_b0".into(), 0), ("m_move".into(), 1), ("m_b1".into(), 2)],
        "the destination's modules must close around the inserted one"
    );
    assert_eq!(
        top_level("p_move_a"),
        vec![("m_stays".into(), 0)],
        "the project being left must not keep a gap in its sort_order"
    );

    // Crucially: the child rows survived the move, and their `root_id`
    // was re-derived with them. Updating only the module row — which is
    // what the pre-`root_id` layout could express — would file the whole
    // subtree under the old project and make it invisible to
    // `list_by_root`.
    let c_after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "c_move"))
        .expect("collection must survive the module move");
    assert_eq!(c_after.parent_id.as_deref(), Some("m_move"));
    assert_eq!(c_after.root_id, "p_move_b");

    let e_after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "e_move"))
        .expect("endpoint must survive the module move");
    assert_eq!(e_after.parent_id.as_deref(), Some("c_move"));
    assert_eq!(e_after.root_id, "p_move_b");

    // The endpoint's request configuration is a separate table now, and
    // must have travelled with it.
    let detail = simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, "e_move"))
        .expect("endpoint detail must survive the module move");
    assert_eq!(detail.url, "/x");
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
async fn endpoint_create_accepts_a_module_parent() {
    // An endpoint with no collection of its own. The matrix names two
    // parent types for an endpoint precisely so this does not need a
    // throwaway collection first — and the MCP tool has to be able to
    // build the same tree the UI can, so `module_id` is accepted
    // alongside `collection_id`.
    use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
    use simple_post_core::domain::{Project, ResourceNode, ResourceNodeType};
    use simple_post_core::permissions::Permission;

    let st = state();

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

    fn node(root: &str, id: &str, kind: ResourceNodeType, parent: Option<&str>) -> ResourceNode {
        ResourceNode {
            id: id.into(),
            root_id: root.into(),
            parent_id: parent.map(String::from),
            kind,
            name: "n".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(
            c,
            &Project {
                id: "p_ep".into(),
                name: "P".into(),
                ..mk()
            },
        )?;
        nodes::create(
            c,
            &node("p_ep", "m_ep", ResourceNodeType::Module, None),
            None,
        )?;
        // The per-project gate defaults to `read_call`, which is below
        // the `full` a create requires.
        permissions::set(c, "p_ep", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = endpoint::tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep",
            "name": "root endpoint",
            "method": "GET",
            "url": "/x"
        }),
    )
    .await
    .expect("creating an endpoint under a module must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    // `EndpointResource` flattens the node, so its fields sit at the top
    // level next to `detail`.
    assert_eq!(created["kind"], "endpoint");
    assert_eq!(created["parentId"], "m_ep");
    assert_eq!(created["rootId"], "p_ep");

    // The detail row has to land with the node, or the tree holds a row
    // the tester cannot open.
    let id = created["id"].as_str().expect("node id").to_string();
    assert_eq!(
        simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, &id))
            .expect("detail row")
            .url,
        "/x"
    );
}

#[tokio::test]
async fn endpoint_create_requires_some_parent() {
    // Neither id supplied: an endpoint with no parent is not a tree, so
    // this is an input error — not a permission denial.
    let res = endpoint::tool_endpoint_create(
        &state(),
        serde_json::json!({
            "name": "n",
            "method": "GET",
            "url": "/x"
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "expected Validation rejection, got {res:?}"
    );
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
