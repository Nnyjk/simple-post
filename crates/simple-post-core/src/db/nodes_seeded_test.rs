// -------------------------------------------------------------------------
// The seeded tree. `fixtures.json` ships exactly one nested collection
// — `c_order_refund`, a child of `c_order_crud`, with an endpoint of
// its own — so both halves of the recursive walk (collection ->
// collection, collection -> endpoint) have a real example. Everything
// else in this file builds its own tree, because the write path needs
// isolating; these read the data a real user would see.
// -------------------------------------------------------------------------

use super::nodes_fixtures::*;
use super::*;
use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;

#[test]
fn seeded_nested_collection_sits_one_level_below_its_parent() {
    // The two depths must be distinguishable: the nested collection is
    // a child of the collection, never of the module it still names in
    // the fixture file.
    let (pool, _dir) = seeded_pool();

    let nested = db::with_conn(&pool, |c| list_children(c, "c_order_crud", None)).unwrap();
    let ids: Vec<&str> = nested.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["c_order_refund", "e_order_list", "e_order_create"],
        "the nested collection is a child of its parent collection, \
         alongside the endpoints that were already there"
    );
    let sub = db::with_conn(&pool, |c| {
        list_children(c, "c_order_crud", Some(ResourceNodeType::Collection))
    })
    .unwrap();
    assert_eq!(sub.len(), 1);
    assert_eq!(sub[0].id, "c_order_refund");
    assert_eq!(sub[0].parent_id.as_deref(), Some("c_order_crud"));

    let top = db::with_conn(&pool, |c| {
        list_children(c, "m_order", Some(ResourceNodeType::Collection))
    })
    .unwrap();
    let ids: Vec<&str> = top.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["c_order_crud", "c_order_status"],
        "the module sees only its own children"
    );
    assert!(
        !ids.contains(&"c_order_refund"),
        "a nested collection must not read as a sibling of its parent"
    );
}

#[test]
fn seeded_nested_collection_and_its_endpoint_inherit_the_ancestors_root_id() {
    // `root_id` is derived, and a future `root_id` trigger would have
    // to re-derive it for the nested rows too. If any of the three
    // disagree the subtree stops being one indexed read.
    let (pool, _dir) = seeded_pool();

    let ancestor = db::with_conn(&pool, |c| root_id_of(c, "c_order_crud")).unwrap();
    assert_eq!(ancestor, "p_demo");
    for id in ["c_order_refund", "e_refund_create"] {
        assert_eq!(
            db::with_conn(&pool, |c| root_id_of(c, id)).unwrap(),
            ancestor,
            "{id} must be filed under the same project as its top-level ancestor"
        );
    }
}

#[test]
fn seeded_nested_collection_is_not_top_level() {
    // `list_by_root` is flat on purpose, so it must still return the
    // nested row — and the `parent_id IS NULL` query the move path
    // uses must not. A nested collection that leaked into the top-level
    // run would be re-rooted on the next sibling renumber.
    let (pool, _dir) = seeded_pool();

    let flat = db::with_conn(&pool, |c| list_by_root(c, "p_demo")).unwrap();
    assert!(
        flat.iter().any(|n| n.id == "c_order_refund"),
        "the flat project read must still carry the nested row"
    );

    // Same shape as `move_many`'s top-level query, which is private.
    let top_level: Vec<String> = db::with_conn(&pool, |c| {
        let mut stmt = c.prepare(
            "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
             ORDER BY sort_order ASC, id ASC",
        )?;
        let ids = stmt
            .query_map(["p_demo"], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    })
    .unwrap();
    assert_eq!(
        top_level,
        vec!["m_auth", "m_order", "m_pay"],
        "only the project's modules are at the top"
    );
}

#[test]
fn seeded_recursive_walk_reaches_through_both_kinds_of_parent_edge() {
    // The recursion has to cross a collection->collection edge and then
    // a collection->endpoint one from a single root.
    let (pool, _dir) = seeded_pool();

    let mut ids = db::with_conn(&pool, |c| subtree_ids(c, &["m_order".to_string()])).unwrap();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "c_order_crud",
            "c_order_refund",
            "c_order_status",
            "e_order_cancel",
            "e_order_create",
            "e_order_list",
            "e_refund_create",
            // The root itself is part of the closure.
            "m_order",
        ]
    );
}

#[test]
fn deleting_a_seeded_parent_takes_the_nested_collection_and_its_endpoint() {
    // Cascading is the schema's job (`parent_id` -> `nodes`, `node_id`
    // -> `endpoint_details`); the assertion is that the seeded nested
    // shape is actually covered by it, detail row included.
    let (pool, _dir) = seeded_pool();

    db::with_tx(&pool, |c| delete(c, "c_order_crud")).unwrap();

    for id in ["c_order_crud", "c_order_refund", "e_refund_create"] {
        let err = db::with_conn(&pool, |c| get(c, id)).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{id}: {err:?}");
    }
    let details: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM endpoint_details WHERE node_id = 'e_refund_create'",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(details, 0, "the detail row cascades with its node");

    assert!(
        db::with_conn(&pool, |c| get(c, "c_order_status")).is_ok(),
        "a sibling of the deleted collection must survive"
    );
}
