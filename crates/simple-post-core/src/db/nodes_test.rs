use super::*;
use crate::db;
use crate::domain::{AuthConfig, Project, RequestBody};
use std::path::PathBuf;

/// 一个含两个项目、树内容为空的库。
/// 经公开 CRUD API 而非 `test_support::seed` 构造，故写路径与类型矩阵被直接考验。需要
/// 出厂树形状的测试改用 [`seeded_pool`]。
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

/// 一个顶层模块：其父是项目，而项目不是节点。
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

/// 一个带着出厂 `fixtures.json` 树的库，含那个嵌套 collection（`c_order_refund`，
/// 挂在 `c_order_crud` 之下）。
/// 下面的测试需要一条真实的 `collection -> collection` 边，否则递归路径在测试数据里
/// 没有任何例子。
fn seeded_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-nodes-seeded-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    (db::test_support::init_seeded(&dir), dir)
}

// 读。

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
    // 这次读是扁平的而非分层的：`sort_order` 以父划定作用域，故一个模块与它的第一个
    // collection 都可以坐在 0 上，树形状由调用方按 parent_id 分桶重建。
    // 必须成立的是：兄弟节点按显示序回来。
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
    // 同一个答案，一次主键命中。
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
fn module_id_of_walks_up_to_the_owning_module() {
    // 值层是 `moduleBaseUrls[service][module]`，故这次查找决定一个 endpoint 拿到该服务的哪一个
    // 地址——嵌套的 endpoint 必须穿过中间那个 collection 抵达它的模块。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p1", "m2", 1), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "m1", 1), None);
    let (e, d) = endpoint("p1", "e1", "c2", 0);
    put(&pool, &e, Some(&d));
    // 直接挂在模块下、中间没有 collection 的 endpoint。
    let (direct, direct_detail) = endpoint("p1", "e2", "m2", 0);
    put(&pool, &direct, Some(&direct_detail));

    for (id, expected) in [("c1", "m1"), ("c2", "m1"), ("e1", "m1"), ("e2", "m2")] {
        let found = db::with_conn(&pool, |c| module_id_of(c, id)).unwrap();
        assert_eq!(found.as_deref(), Some(expected), "module_id_of({id})");
    }
}

#[test]
fn module_id_of_is_none_for_a_module_or_an_unknown_node() {
    // 两者都表示「这上面没有模块」，且都解析不到 base URL——但它们是不同的状态，故都不是错误。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    assert_eq!(
        db::with_conn(&pool, |c| module_id_of(c, "m1")).unwrap(),
        None,
        "a module is not under a module"
    );
    assert_eq!(
        db::with_conn(&pool, |c| module_id_of(c, "nope")).unwrap(),
        None
    );
}

#[test]
fn module_id_of_follows_an_endpoint_that_was_dragged_to_another_module() {
    // 服务现在是项目级的，故跨模块拖拽无需改指 endpoint 的服务——但它解析所经过的**单元格**
    // 跟着它走，而这次查找就是在测那个。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &module("p1", "m2", 1), None);
    let (e, d) = endpoint("p1", "e1", "m1", 0);
    put(&pool, &e, Some(&d));

    db::with_tx(&pool, |c| move_many(c, &["e1".to_string()], Some("m2"), 0)).unwrap();
    let found = db::with_conn(&pool, |c| module_id_of(c, "e1")).unwrap();
    assert_eq!(found.as_deref(), Some("m2"));
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

// 写。

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

// 移动。

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

// 触发器。应用层才是主要的检查；这些测试证明一次绕过它的写入仍造不出走不动的树。

#[test]
fn a_trigger_rejects_an_endpoint_under_an_endpoint() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    // endpoint 是叶子。它被拓宽的父集合是 module / collection，绝不是另一个 endpoint——
    // 而对一个裸写入者来说，守住这条线的是触发器而不是应用层。
    db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e_raw', 'p1', 'm1', 'endpoint', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .expect("an endpoint under a module is legal");
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e_child', 'p1', 'e_raw', 'endpoint', 'raw', 0, 1, 1)",
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
    // 一个对一切都开火的触发器比没有更糟：它把调用方推向关掉外键。
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
fn the_trigger_allows_an_endpoint_under_a_module() {
    // update 触发器也带着同一份矩阵，且与它一起被拓宽了。留在旧谓词上，它会中止应用层刚刚
    // 认可的那次改父——而 `module_move` / `endpoint_move` 直穿它。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    // 把它从 collection 里抬出来，走裸 SQL。
    db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'm1' WHERE id = 'e1'", [])?;
        Ok(())
    })
    .expect("reparenting an endpoint to a module is legal");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("m1")
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

// 搜索与复制。两者都是取代 `/api/endpoints/search` 与
// `/api/endpoints/:id/duplicate` 的读路径所需要的。

#[test]
fn search_matches_node_name() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m_auth", 0), None);
    put(&pool, &collection("p1", "c_login", "m_auth", 0), None);

    let got = db::with_conn(&pool, |c| search(c, "login")).unwrap();
    let ids: Vec<&str> = got.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["c_login"]);
}

#[test]
fn search_reaches_into_endpoint_url() {
    // `url` lives in endpoint_details, which is exactly why the search
    // has to join rather than match on the node alone.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (mut e, d) = endpoint("p1", "e1", "c1", 0);
    e.name = "Fetch one".into();
    let mut d = d;
    d.url = "/users/:id/profile".into();
    put(&pool, &e, Some(&d));

    let got = db::with_conn(&pool, |c| search(c, "profile")).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, "e1");
}

#[test]
fn search_does_not_match_a_null_url_on_a_non_endpoint() {
    // Without the `n.type = 'endpoint'` guard, every module and
    // collection would match a LIKE against its own NULL url column.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    assert!(db::with_conn(&pool, |c| search(c, "e")).unwrap().is_empty());
}

#[test]
fn duplicate_copies_the_endpoint_and_its_configuration() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (mut e, d) = endpoint("p1", "e1", "c1", 0);
    e.name = "Fetch one".into();
    let mut d = d;
    d.url = "/users".into();
    d.tags = vec!["a".into()];
    put(&pool, &e, Some(&d));

    let copy = db::with_tx(&pool, |c| duplicate(c, "e1", None)).unwrap();
    assert_ne!(copy.id, "e1", "a copy needs its own id");
    assert_eq!(copy.parent_id.as_deref(), Some("c1"), "same parent");
    assert_eq!(copy.name, "Fetch one (副本)");
    assert_eq!(
        copy.sort_order,
        e.sort_order + 1,
        "the copy lands next to the original, not at the tail"
    );

    let detail = db::with_conn(&pool, |c| crate::db::endpoint_details::get(c, &copy.id)).unwrap();
    assert_eq!(detail.node_id, copy.id);
    assert_eq!(detail.url, "/users");
    assert_eq!(detail.tags, vec!["a".to_string()]);
}

#[test]
fn duplicate_refuses_a_non_endpoint() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    for id in ["m1", "c1"] {
        let err = db::with_tx(&pool, |c| duplicate(c, id, None)).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)), "{id}: {err:?}");
    }
}

#[test]
fn duplicate_adopts_the_id_the_caller_chose() {
    // The SPA duplicates optimistically: it activates the tab and starts
    // a rename against this id *before* the call returns, so an id minted
    // here would leave every one of those pointing at a row that does not
    // exist. `new_id` is therefore adopted verbatim, and `create` stays
    // the place that refuses a collision.
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let copy = db::with_tx(&pool, |c| duplicate(c, "e1", Some("picked-here"))).unwrap();
    assert_eq!(copy.id, "picked-here", "the caller's id is the row's id");
    assert_eq!(
        copy.name, "e1 (副本)",
        "the suffix is the UI's, not an English one"
    );

    // The detail row has to follow the same id, or the copy is a node the
    // HTTP tester cannot open.
    let detail = db::with_conn(&pool, |c| {
        crate::db::endpoint_details::get(c, "picked-here")
    })
    .unwrap();
    assert_eq!(detail.node_id, "picked-here");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "picked-here")).unwrap().id,
        "picked-here"
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

// -------------------------------------------------------------------------
// The seeded tree. `fixtures.json` ships exactly one nested collection
// — `c_order_refund`, a child of `c_order_crud`, with an endpoint of
// its own — so both halves of the recursive walk (collection ->
// collection, collection -> endpoint) have a real example. Everything
// else in this file builds its own tree, because the write path needs
// isolating; these read the data a real user would see.
// -------------------------------------------------------------------------

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
