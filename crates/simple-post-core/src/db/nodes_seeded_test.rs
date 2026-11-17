// -------------------------------------------------------------------------
// 播种数据的那棵树。`fixtures.json` 只带一个嵌套集合——`c_order_refund`，它是
// `c_order_crud` 的孩子、自己带一个端点——递归遍历的两段边（集合→集合、集合→端点）
// 因此各有一个真实样本。本文件其余用例自建树，因为写路径需要隔离；读播种数据的
// 那三条只读真实用户会看到的内容。
// -------------------------------------------------------------------------

use super::nodes_fixtures::*;
use super::*;
use crate::db;
use crate::domain::ResourceNodeType;
use crate::error::AppError;

/// 播种出来的每一个端点，它的配置行都必须**读得出来**。
///
/// `fixtures.json` 的 `auth` 必须用 `kind` 字段：`AuthConfig` 按 `kind` 反序列化
/// `auth_json`。树形测试只读 `nodes` 表，不读 `auth_json`，故本测试逐个读出配置行。
#[test]
fn every_seeded_endpoint_has_a_readable_detail_row() {
    let (pool, _dir) = seeded_pool();

    let endpoints = db::with_conn(&pool, |c| {
        list_by_root_and_type(c, "p_demo", ResourceNodeType::Endpoint)
    })
    .unwrap();
    assert!(
        !endpoints.is_empty(),
        "fixture 里得有端点，否则这条断言什么也验不到"
    );

    for node in endpoints {
        let detail = db::with_conn(&pool, |c| crate::db::endpoint_details::get(c, &node.id))
            .unwrap_or_else(|e| panic!("端点 {} 的配置行读不出来：{e}", node.id));
        assert!(!detail.url.is_empty(), "{} 的 url 不该是空的", node.id);
    }
}

#[test]
fn seeded_nested_collection_sits_one_level_below_its_parent() {
    // 两层深度必须分得开：嵌套集合是那个集合的孩子，不是 `fixtures.json` 里
    // 它所在模块的孩子。
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
    // `root_id` 是派生列：嵌套行也必须派生到同一个值，三行里只要有一个不一致，
    // 这棵子树就不再是一次索引读能取到的整体。
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
    // `list_by_root` 刻意做成扁平读取，嵌套行照样返回；移动路径那句
    // `parent_id IS NULL` 的查询则不许返回它：否则下次兄弟重排会把嵌套集合的根
    // 改到别处。
    let (pool, _dir) = seeded_pool();

    let flat = db::with_conn(&pool, |c| list_by_root(c, "p_demo")).unwrap();
    assert!(
        flat.iter().any(|n| n.id == "c_order_refund"),
        "the flat project read must still carry the nested row"
    );

    // 与 `move_many` 里那句私有顶层查询同一个形状。
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
    // 递归必须从同一个根穿过集合→集合、集合→端点两种父边。
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
            // 根本身也在闭包里。
            "m_order",
        ]
    );
}

#[test]
fn deleting_a_seeded_parent_takes_the_nested_collection_and_its_endpoint() {
    // 级联删除由 schema 的外键完成（`parent_id` → `nodes`，`node_id` →
    // `endpoint_details`）；这里断言播种出来的嵌套形状确实被覆盖，配置行也算在内。
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
