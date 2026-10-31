// 读：列表、单行查找、向上溯源、子树闭包、搜索与复制。

use super::nodes_fixtures::*;
use super::*;
use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;

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
    // 值层是 `moduleBaseUrls[module]`，故这次查找决定一个 endpoint 取哪一格的地址
    // ——嵌套的 endpoint 必须穿过中间那个 collection 抵达它的模块。
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
    // 地址住在 (环境 × 模块) 那一格里，故端点被拖到别的模块下时，它解析的那一格跟着
    // 端点走，而这次查找就是在测那个。
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

// ---------------------------------------------------------------------------
// 共享样本：集合子树的端点清单与顺序
// ---------------------------------------------------------------------------
//
// 顺序规则由一份 JSON 钉住，TS 侧（应用内批量运行的 `collect`）读同一份文件。
// 两侧各写一遍遍历，故只有一份共享样本才能钉住它们没分叉。

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const COLLECTION_ORDER_FIXTURE: &str =
    include_str!("../../../../fixtures/collection-order-parity.json");

/// `deny_unknown_fields` 与 `rename_all` 一起上：前者把「键名对不上」从静默忽略变成
/// 响亮失败——serde 默认会忽略结构体里没有的键，于是样本里改了字段名而读取器没跟上时，
/// 那条断言会一条都没跑到却一直是绿的。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CollectionOrderFixture {
    #[serde(rename = "$comment")]
    _comment: Option<String>,
    cases: Vec<CollectionOrderCase>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CollectionOrderCase {
    name: String,
    root: String,
    nodes: Vec<CollectionOrderNode>,
    expected: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CollectionOrderNode {
    id: String,
    root_id: String,
    parent_id: Option<String>,
    kind: String,
    name: String,
    sort_order: i64,
}

#[test]
fn ordered_subtree_endpoints_matches_the_shared_sample() {
    let fixture: CollectionOrderFixture = serde_json::from_str(COLLECTION_ORDER_FIXTURE)
        .expect("fixtures/collection-order-parity.json must parse");
    assert!(
        !fixture.cases.is_empty(),
        "the shared sample must not be empty — an empty one pins nothing"
    );
    for case in &fixture.cases {
        let (pool, _dir) = fresh_pool();
        seed_collection_order_case(&pool, case);
        let got = db::with_conn(&pool, |c| ordered_subtree_endpoints(c, &case.root))
            .unwrap_or_else(|e| panic!("case {:?}: {e}", case.name));
        let ids: Vec<&str> = got.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, case.expected, "case: {}", case.name);
    }
}

/// 把样本里的一棵树写进一个临时库。
///
/// 节点按「父先于子」插入：`nodes.parent_id` 是自引用外键，父不存在时插入会被拒。
fn seed_collection_order_case(pool: &db::DbPool, case: &CollectionOrderCase) {
    // 样本里用到的每个项目都要有一行，否则外键拒绝。
    let mut roots: Vec<&str> = case.nodes.iter().map(|n| n.root_id.as_str()).collect();
    roots.sort_unstable();
    roots.dedup();
    for root in roots {
        let exists = db::with_conn(pool, |c| Ok(db::projects::get(c, root).is_ok())).unwrap();
        if exists {
            continue;
        }
        db::with_tx(pool, |c| {
            db::projects::create(
                c,
                &crate::domain::Project {
                    id: root.into(),
                    name: root.into(),
                    description: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )
        })
        .unwrap();
    }

    let mut pending: Vec<&CollectionOrderNode> = case.nodes.iter().collect();
    let mut inserted: std::collections::HashSet<String> = std::collections::HashSet::new();
    while !pending.is_empty() {
        let mut rest: Vec<&CollectionOrderNode> = Vec::new();
        let mut progressed = false;
        for node in pending {
            let ready = node
                .parent_id
                .as_deref()
                .is_none_or(|parent| inserted.contains(parent));
            if !ready {
                rest.push(node);
                continue;
            }
            let kind = ResourceNodeType::parse(&node.kind)
                .unwrap_or_else(|| panic!("unknown node kind {:?}", node.kind));
            let resource = crate::domain::ResourceNode {
                id: node.id.clone(),
                root_id: node.root_id.clone(),
                parent_id: node.parent_id.clone(),
                kind,
                name: node.name.clone(),
                description: None,
                sort_order: node.sort_order,
                created_at: 1,
                updated_at: 1,
            };
            let detail_row = match kind {
                ResourceNodeType::Endpoint => Some(detail(&node.id)),
                _ => None,
            };
            put(pool, &resource, detail_row.as_ref());
            inserted.insert(node.id.clone());
            progressed = true;
        }
        assert!(
            progressed,
            "case {:?} has a node whose parent is not in the sample",
            case.name
        );
        pending = rest;
    }
}
