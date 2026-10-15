// 移动：`move_many`、`move_to_root` 与直接子节点读取。

use super::nodes_fixtures::*;
use super::*;
use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;

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

    // endpoint 不是任何东西的合法父。
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
    // 促使重新推导 root_id 的那个回归：只指名被拖动的行会让子树仍归档在旧项目下，
    // 于是重载时它从新项目的树上消失。
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

// `move_to_root`——`move_many` 表达不了的那一种移动，因为没有父节点时就没有可读的新
// `root_id` 之处。

/// 一个项目的顶层模块，按显示序的 `(id, sort_order)`。
fn top_level(pool: &db::DbPool, root: &str) -> Vec<(String, i64)> {
    db::with_conn(pool, |c| {
        list_by_root_and_type(c, root, ResourceNodeType::Module)
    })
    .unwrap()
    .into_iter()
    .map(|n| (n.id, n.sort_order))
    .collect()
}

#[test]
fn move_to_root_reassigns_the_whole_subtree() {
    // 这一形状招来的 bug：只更新指名的行会把 collection 与 endpoint 留在旧项目下，
    // `list_by_root` 从此再也不显示它们——与旧布局里 collection `module_id` 的漂移同一类。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p2", "m2", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    db::with_tx(&pool, |c| move_to_root(c, "m1", "p2", None)).unwrap();

    for id in ["m1", "c1", "e1"] {
        assert_eq!(
            db::with_conn(&pool, |c| get(c, id)).unwrap().root_id,
            "p2",
            "{id} must follow the module across the project boundary"
        );
    }
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("c1")
    );
    // endpoint 的配置在另一张表里；它绝不能被落下。
    assert_eq!(
        db::with_conn(&pool, |c| super::super::endpoint_details::get(c, "e1"))
            .unwrap()
            .url,
        "/e1"
    );
}

#[test]
fn move_to_root_closes_the_gap_it_leaves_behind() {
    // 回归网。压缩那一遍跑在离开中的模块**仍**归档在旧 root 下时，故把它算进去会让它腾出的
    // 槽位继续占着，什么也不上移。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "a", 0), None);
    put(&pool, &module("p1", "b", 1), None);
    put(&pool, &module("p1", "c", 2), None);
    put(&pool, &module("p2", "x", 0), None);

    db::with_tx(&pool, |c| move_to_root(c, "a", "p2", None)).unwrap();

    assert_eq!(
        top_level(&pool, "p1"),
        vec![("b".into(), 0), ("c".into(), 1)],
        "the project being left must not keep a gap in its sort_order"
    );
    assert_eq!(
        top_level(&pool, "p2"),
        vec![("x".into(), 0), ("a".into(), 1)]
    );
}

#[test]
fn move_to_root_splices_into_the_destination_at_the_requested_slot() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "a", 0), None);
    put(&pool, &module("p2", "x", 0), None);
    put(&pool, &module("p2", "y", 1), None);
    put(&pool, &module("p2", "z", 2), None);

    db::with_tx(&pool, |c| move_to_root(c, "a", "p2", Some(1))).unwrap();

    assert_eq!(
        top_level(&pool, "p2"),
        vec![
            ("x".into(), 0),
            ("a".into(), 1),
            ("y".into(), 2),
            ("z".into(), 3)
        ],
        "the destination's modules must close around the inserted one"
    );
}

#[test]
fn move_to_root_clamps_an_index_past_the_end() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "a", 0), None);
    put(&pool, &module("p2", "x", 0), None);

    db::with_tx(&pool, |c| move_to_root(c, "a", "p2", Some(99))).unwrap();

    assert_eq!(
        top_level(&pool, "p2"),
        vec![("x".into(), 0), ("a".into(), 1)]
    );
}

#[test]
fn move_to_root_rejects_a_non_module() {
    // collection 或 endpoint 换项目是一次**移动**，不是转移：它的项目由父链决定，
    // 静默改根会让它脱离自己的同级。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);

    let err = db::with_tx(&pool, |c| move_to_root(c, "c1", "p2", None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
}

#[test]
fn move_to_root_rejects_an_unknown_project() {
    // 否则子树会被归档到一个没有任何项目行支撑的 root 下，此后永远没人能列出它。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);

    let err = db::with_tx(&pool, |c| move_to_root(c, "m1", "ghost", None)).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "m1")).unwrap().root_id,
        "p1",
        "a rejected transfer must not have moved anything"
    );
}

#[test]
fn list_children_returns_direct_children_only() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "m1", 1), None);
    put(&pool, &collection("p1", "c1a", "c1", 0), None);

    let direct = db::with_conn(&pool, |c| list_children(c, "m1", None)).unwrap();
    let mut ids = direct.iter().map(|n| n.id.as_str()).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["c1", "c2"], "c1a is a grandchild, not a child");

    let only_collections = db::with_conn(&pool, |c| {
        list_children(c, "m1", Some(ResourceNodeType::Collection))
    })
    .unwrap();
    assert_eq!(only_collections.len(), 2);

    let leaf = db::with_conn(&pool, |c| list_children(c, "c1", None)).unwrap();
    assert_eq!(leaf.len(), 1);
    assert_eq!(leaf[0].id, "c1a");
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
