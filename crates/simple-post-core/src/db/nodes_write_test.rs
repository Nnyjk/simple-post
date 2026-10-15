// 写：插入、父子矩阵与级联删除。

use super::nodes_fixtures::*;
use super::*;
use crate::db;
use crate::domain::{ResourceNode, ResourceNodeType};
use crate::error::AppError;

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
    // 文档描述的是四层树。这条断言说的是另一回事：嵌套无上限，且一直如此。
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

// 父子矩阵。这些过去全靠外键保证。

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
fn an_endpoint_may_hang_off_a_module_directly() {
    // 单个 endpoint 的情形。要求一个 collection 曾让这种最简情况先得为此建一个用完即弃的
    // 文件夹，故矩阵给 endpoint 命名两种父类型、只给 collection 一种。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "m1", 0);
    put(&pool, &e, Some(&d));

    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.kind, ResourceNodeType::Endpoint);
    assert_eq!(got.parent_id.as_deref(), Some("m1"));
    assert_eq!(got.root_id, "p1");
    // detail 行必须与节点一起落地，否则树上会有一行测试者打不开的东西。
    assert_eq!(
        db::with_conn(&pool, |c| db::endpoint_details::get(c, "e1"))
            .unwrap()
            .url,
        "/e1"
    );
}

#[test]
fn an_endpoint_may_still_hang_off_a_collection() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("c1")
    );
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
fn a_top_level_node_naming_no_project_is_a_validation_error() {
    // 两条传输层都把缺席的 `rootId` 默认成空串，而模块没有 `parentId` 可供推导——故空串会直达
    // `INSERT` 并以 `FOREIGN KEY constraint failed` 回来。在 REST 面上那是一个 500 加
    // `code: "sqlite"`，客户端无法把它与真正的服务端故障区分开。它是调用方的错误，
    // 就必须读起来像调用方的错误。
    let (pool, _dir) = fresh_pool();
    let nameless = module("", "m1", 0);
    let err = db::with_tx(&pool, |c| create(c, &nameless, None)).unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected a validation error, got {err:?}"
    );
    assert!(
        !matches!(err, AppError::Sqlite(_)),
        "must not surface as a transport failure"
    );
}

#[test]
fn a_top_level_node_naming_an_unknown_project_is_a_validation_error() {
    // 同一个洞的另一半：`rootId` 打错字同样会以外键失败回来。
    let (pool, _dir) = fresh_pool();
    let wrong = module("p_does_not_exist", "m1", 0);
    let err = db::with_tx(&pool, |c| create(c, &wrong, None)).unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected a validation error, got {err:?}"
    );
}

#[test]
fn root_id_must_match_the_parents() {
    // 一个归档在一个项目下、却在另一个项目的树里渲染的节点，对它自称的那个项目而言
    // `list_by_root` 是看不见的。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let mismatched = collection("p2", "c1", "m1", 0);
    let err = db::with_tx(&pool, |c| create(c, &mismatched, None)).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
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
