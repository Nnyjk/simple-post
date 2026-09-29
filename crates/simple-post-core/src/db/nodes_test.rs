use super::*;
use crate::db;
use crate::domain::{AuthConfig, Project, RequestBody};
use std::path::PathBuf;

/// A database with two projects and no tree content.
///
/// Built through the public CRUD API rather than `test_support::seed`,
/// because the fixtures still describe the old three-table layout and
/// populate none of `nodes`.
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-nodes-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    for (id, name) in [("p1", "First"), ("p2", "Second")] {
        db::with_tx(&pool, |c| {
            db::projects::create(
                c,
                &Project {
                    id: id.into(),
                    name: name.into(),
                    description: None,
                    default_base_url_definition_id: None,
                    base_url_definitions: vec![],
                    base_url: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )
        })
        .unwrap();
    }
    (pool, dir)
}

fn node(
    root: &str,
    id: &str,
    kind: ResourceNodeType,
    parent: Option<&str>,
    sort: i64,
) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: root.into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: sort,
        created_at: 1,
        updated_at: 1,
    }
}

/// A top-level module: its parent is the project, which is not a node.
fn module(root: &str, id: &str, sort: i64) -> ResourceNode {
    node(root, id, ResourceNodeType::Module, None, sort)
}

fn collection(root: &str, id: &str, parent: &str, sort: i64) -> ResourceNode {
    node(root, id, ResourceNodeType::Collection, Some(parent), sort)
}

fn endpoint(root: &str, id: &str, parent: &str, sort: i64) -> (ResourceNode, EndpointDetail) {
    let d = detail(id);
    (
        node(root, id, ResourceNodeType::Endpoint, Some(parent), sort),
        d,
    )
}

fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: crate::domain::HttpMethod::Get,
        url: format!("/{node_id}"),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
    }
}

fn put(pool: &db::DbPool, n: &ResourceNode, d: Option<&EndpointDetail>) {
    db::with_tx(pool, |c| create(c, n, d)).expect("create node");
}

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

#[test]
fn list_by_root_returns_only_that_projects_tree() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p2", "m2", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);

    let got = db::with_conn(&pool, |c| list_by_root(c, "p1")).unwrap();
    let mut ids: Vec<&str> = got.iter().map(|n| n.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["c1", "m1"], "p2's module must not leak into p1");
}

#[test]
fn list_by_root_orders_siblings_by_sort_order() {
    // The read is flat, not hierarchical: `sort_order` is scoped to a
    // parent, so a module and its first collection can both sit at 0 and
    // the tree shape is rebuilt by the caller bucketing on parent_id.
    // What has to hold is that siblings come back in display order.
    // (`list_by_module` on the old layout behaved the same way.)
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "third", "m1", 2), None);
    put(&pool, &collection("p1", "first", "m1", 0), None);
    put(&pool, &collection("p1", "second", "m1", 1), None);

    let got = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p1", ResourceNodeType::Collection)
    })
    .unwrap();
    let ids: Vec<&str> = got.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["first", "second", "third"]);
}

#[test]
fn list_by_root_and_type_filters() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let endpoints = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p1", ResourceNodeType::Endpoint)
    })
    .unwrap();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].id, "e1");

    let modules = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p1", ResourceNodeType::Module)
    })
    .unwrap();
    assert_eq!(modules.len(), 1);
}

#[test]
fn root_id_of_is_one_lookup_for_every_type() {
    // The old layout walked collections -> modules for this. Same
    // answer, one primary-key hit.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    for id in ["m1", "c1", "e1"] {
        assert_eq!(
            db::with_conn(&pool, |c| root_id_of(c, id)).unwrap(),
            "p1",
            "root_id_of({id})"
        );
    }
    let err = db::with_conn(&pool, |c| root_id_of(c, "nope")).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
}

#[test]
fn subtree_ids_walks_the_whole_closure() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);
    put(&pool, &collection("p1", "c3", "c2", 0), None);
    put(&pool, &collection("p1", "other", "m1", 1), None);

    let got = db::with_conn(&pool, |c| subtree_ids(c, &["c1".to_string()])).unwrap();
    let mut got = got;
    got.sort();
    assert_eq!(got, vec!["c1", "c2", "c3"]);
}

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

#[test]
fn each_type_can_be_created() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.kind, ResourceNodeType::Endpoint);
    assert_eq!(got.parent_id.as_deref(), Some("c1"));
    assert_eq!(got.root_id, "p1");
}

#[test]
fn collections_nest_without_a_depth_limit_in_the_type() {
    // The docs describe a four-level tree. This is the assertion that
    // says otherwise: nesting is unbounded and always worked.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let mut parent = "m1".to_string();
    for i in 0..8 {
        let id = format!("c{i}");
        put(&pool, &collection("p1", &id, &parent, 0), None);
        parent = id;
    }
    let got = db::with_conn(&pool, |c| get(c, "c7")).unwrap();
    assert_eq!(got.parent_id.as_deref(), Some("c6"));
}

#[test]
fn endpoint_without_a_detail_payload_is_rejected() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, _d) = endpoint("p1", "e1", "c1", 0);
    let err = db::with_tx(&pool, |c| create(c, &e, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn detail_payload_on_a_non_endpoint_is_rejected() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let c = collection("p1", "c1", "m1", 0);
    let d = detail("c1");
    let err = db::with_tx(&pool, |x| create(x, &c, Some(&d))).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

// -------------------------------------------------------------------------
// Parent / child matrix. Foreign keys used to guarantee all of this.
// -------------------------------------------------------------------------

#[test]
fn a_module_may_not_have_a_node_parent() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let nested = ResourceNode {
        parent_id: Some("m1".into()),
        ..module("p1", "m2", 1)
    };
    let err = db::with_tx(&pool, |c| create(c, &nested, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn an_endpoint_may_not_hang_off_a_module() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "m1", 0);
    let err = db::with_tx(&pool, |c| create(c, &e, Some(&d))).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn a_collection_may_not_hang_off_an_endpoint() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let bad = collection("p1", "c2", "e1", 0);
    let err = db::with_tx(&pool, |c| create(c, &bad, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn a_non_module_may_not_sit_at_the_top_of_a_project() {
    let (pool, _dir) = fresh_pool();
    let orphan = node("p1", "c1", ResourceNodeType::Collection, None, 0);
    let err = db::with_tx(&pool, |c| create(c, &orphan, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn root_id_must_match_the_parents() {
    // A node filed under one project while rendering inside another's
    // tree is invisible to list_by_root for the project it claims.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let mismatched = collection("p2", "c1", "m1", 0);
    let err = db::with_tx(&pool, |c| create(c, &mismatched, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

// -------------------------------------------------------------------------
// Moving.
// -------------------------------------------------------------------------

#[test]
fn a_node_may_not_move_under_its_own_descendant() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);
    put(&pool, &collection("p1", "c3", "c2", 0), None);

    let err = db::with_tx(&pool, |c| move_many(c, &["c1".to_string()], Some("c3"), 0)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn move_rejects_an_illegal_parent_type() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    // An endpoint is not a legal parent for anything.
    let err = db::with_tx(&pool, |c| move_many(c, &["c1".to_string()], Some("e1"), 0)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn move_renumbers_the_destination_without_gaps() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "a", "m1", 0), None);
    put(&pool, &collection("p1", "b", "m1", 1), None);
    put(&pool, &collection("p1", "c", "m1", 2), None);

    db::with_tx(&pool, |c| move_many(c, &["c".to_string()], Some("m1"), 0)).unwrap();

    let children = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p1", ResourceNodeType::Collection)
    })
    .unwrap();
    let orders: Vec<i64> = children.iter().map(|n| n.sort_order).collect();
    assert_eq!(orders, vec![0, 1, 2], "siblings must be a contiguous run");
    assert_eq!(children[0].id, "c", "caller order decides the slot");
}

#[test]
fn move_across_projects_reassigns_the_whole_subtree() {
    // The regression that motivated re-deriving root_id at all: naming
    // only the dragged rows leaves the subtree filed under the old
    // project, so it disappears from the new project's tree on reload.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p2", "m2", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);
    put(&pool, &collection("p1", "c3", "c2", 0), None);
    let (e, d) = endpoint("p1", "e1", "c3", 0);
    put(&pool, &e, Some(&d));

    db::with_tx(&pool, |c| move_many(c, &["c1".to_string()], Some("m2"), 0)).unwrap();

    for id in ["c1", "c2", "c3", "e1"] {
        assert_eq!(
            db::with_conn(&pool, |c| root_id_of(c, id)).unwrap(),
            "p2",
            "{id} must follow the move"
        );
    }
    let p1 = db::with_conn(&pool, |c| list_by_root(c, "p1")).unwrap();
    assert!(p1.iter().all(|n| n.id == "m1"), "p1 keeps only its module");
    let p2 = db::with_conn(&pool, |c| list_by_root(c, "p2")).unwrap();
    assert_eq!(p2.len(), 5, "the whole subtree moved to p2");
}

#[test]
fn a_within_project_move_leaves_the_rest_of_the_tree_untouched() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p1", "m2", 1), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "m2", 0), None);

    db::with_tx(&pool, |c| move_many(c, &["c1".to_string()], Some("m2"), 0)).unwrap();

    assert_eq!(
        db::with_conn(&pool, |c| get(c, "c1")).unwrap().root_id,
        "p1"
    );
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "c2"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("m2"),
        "a sibling that did not move keeps its slot"
    );
}

#[test]
fn multi_select_keeps_caller_order() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "a", "m1", 0), None);
    put(&pool, &collection("p1", "b", "m1", 1), None);
    put(&pool, &collection("p1", "c", "m1", 2), None);

    db::with_tx(&pool, |c| {
        move_many(c, &["c".to_string(), "a".to_string()], Some("m1"), 0)
    })
    .unwrap();

    let children = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p1", ResourceNodeType::Collection)
    })
    .unwrap();
    let ids: Vec<&str> = children.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["c", "a", "b"], "the caller's pick order wins");
}

// -------------------------------------------------------------------------
// Triggers. The application layer is the primary check; these prove a
// write that bypasses it still cannot produce an unwalkable tree.
// -------------------------------------------------------------------------

#[test]
fn a_trigger_rejects_an_endpoint_under_a_module() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    // Raw INSERT, straight past `db::nodes::create`.
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e_raw', 'p1', 'm1', 'endpoint', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("parent type does not accept"),
        "the trigger must be what stops this, got {msg:?}"
    );
}

#[test]
fn a_trigger_rejects_a_module_with_a_node_parent() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('m_raw', 'p1', 'm1', 'module', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("a module has no node parent"),
        "got {:?}",
        err.to_string()
    );
}

#[test]
fn a_trigger_rejects_an_illegal_reparent() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let err = db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'e1' WHERE id = 'c1'", [])?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("parent type does not accept"),
        "got {:?}",
        err.to_string()
    );
}

#[test]
fn the_trigger_allows_what_the_matrix_allows() {
    // A trigger that fires on everything is worse than none: it pushes
    // callers toward disabling foreign keys.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);

    db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'c1' WHERE id = 'c2'", [])?;
        Ok(())
    })
    .expect("collection under a collection is legal");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "c2"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("c1")
    );
}

#[test]
fn the_check_constraint_holds_the_type_set_closed() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('x', 'p1', NULL, 'folder', 'x', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("check"),
        "the CHECK constraint should reject an unknown type, got {:?}",
        err.to_string()
    );
}

// -------------------------------------------------------------------------
// Cascade delete.
// -------------------------------------------------------------------------

#[test]
fn deleting_a_collection_takes_its_subtree_and_endpoints() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c2", 0);
    put(&pool, &e, Some(&d));

    db::with_tx(&pool, |c| delete(c, "c1")).unwrap();

    let left = db::with_conn(&pool, |c| list_by_root(c, "p1")).unwrap();
    let ids: Vec<&str> = left.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["m1"], "c1, c2 and e1 all go");
    let details: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM endpoint_details", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(details, 0, "the detail row cascades with its node");
}

#[test]
fn deleting_a_project_takes_the_whole_tree() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    db::with_tx(&pool, |c| db::projects::delete(c, "p1")).unwrap();

    let left: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(left, 0, "root_id's cascade removes the tree in one delete");
}

#[test]
fn deep_nesting_survives_a_cascade_delete() {
    // The old layout had no depth ceiling on collection nesting, so the
    // merge must not introduce one. 200 is well past anything a user
    // builds and well under SQLite's recursion ceiling.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let mut parent = "m1".to_string();
    for i in 0..200 {
        let id = format!("c{i}");
        put(&pool, &collection("p1", &id, &parent, 0), None);
        parent = id;
    }

    let depth = db::with_conn(&pool, |c| subtree_ids(c, &["c0".to_string()])).unwrap();
    assert_eq!(depth.len(), 200);

    db::with_tx(&pool, |c| delete(c, "c0")).unwrap();
    let left: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(left, 1, "only the module survives");
}
