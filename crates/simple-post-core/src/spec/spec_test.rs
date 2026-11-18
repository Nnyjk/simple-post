//! `core::spec` 的测试：按名寻址与幂等复用。

use crate::db::test_helpers::fresh_pool;
use crate::db::{self, DbPool};
use crate::domain::ResourceNodeType;
use crate::spec::{self, IfExists};

// =========================================================================
// IfExists
// =========================================================================

#[test]
fn if_exists_parse_covers_the_three_modes_and_the_default() {
    assert_eq!(IfExists::parse("").unwrap(), IfExists::Reuse);
    assert_eq!(IfExists::parse("reuse").unwrap(), IfExists::Reuse);
    assert_eq!(IfExists::parse("error").unwrap(), IfExists::Error);
    assert_eq!(IfExists::parse("duplicate").unwrap(), IfExists::Duplicate);
}

#[test]
fn if_exists_parse_rejects_an_unknown_mode() {
    let e = IfExists::parse("skip").unwrap_err();
    assert!(
        e.to_string().contains("skip"),
        "错误信息里要点名那个不认识的取值：{e}"
    );
}

// =========================================================================
// 模块
// =========================================================================

#[test]
fn resolve_module_reuses_the_existing_one_by_name() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // fixture 里 p_demo 下已经有一个叫「订单」的模块（m_order）。
        let r = spec::resolve_module(c, "p_demo", "订单", IfExists::Reuse).unwrap();
        assert_eq!(r.id, "m_order");
        assert!(!r.created, "已有的模块不该被重新建一遍");
        Ok(())
    })
    .unwrap();
}

#[test]
fn resolve_module_creates_when_the_name_is_new() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        let r = spec::resolve_module(c, "p_demo", "营销", IfExists::Reuse).unwrap();
        assert!(r.created);
        assert_ne!(r.id, "m_auth");
        let node = db::nodes::get(c, &r.id).unwrap();
        assert_eq!(node.kind, ResourceNodeType::Module);
        assert_eq!(node.name, "营销");
        assert_eq!(node.root_id, "p_demo");
        assert!(node.parent_id.is_none(), "模块的父级是项目，不是另一个节点");
        Ok(())
    })
    .unwrap();
}

#[test]
fn resolve_module_reuse_is_idempotent_across_calls() {
    let (pool, _dir) = fresh_pool();
    let first = db::with_conn(&pool, |c| {
        spec::resolve_module(c, "p_demo", "营销", IfExists::Reuse).map(|r| r.id)
    })
    .unwrap();
    let second = db::with_conn(&pool, |c| {
        spec::resolve_module(c, "p_demo", "营销", IfExists::Reuse).map(|r| r.id)
    })
    .unwrap();
    assert_eq!(first, second, "同一次寻址重复调用只能得到同一个模块");
}

#[test]
fn resolve_module_error_mode_refuses_to_build_a_second_one() {
    let (pool, _dir) = fresh_pool();
    let e = db::with_conn(&pool, |c| {
        spec::resolve_module(c, "p_demo", "订单", IfExists::Error)
    })
    .unwrap_err();
    assert!(e.to_string().contains("订单"), "{e}");
}

#[test]
fn resolve_module_duplicate_mode_builds_a_second_one() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        let r = spec::resolve_module(c, "p_demo", "订单", IfExists::Duplicate).unwrap();
        assert!(r.created);
        assert_ne!(r.id, "m_order");
        Ok(())
    })
    .unwrap();
}

#[test]
fn resolve_module_mints_ids_that_do_not_collide_in_the_same_millisecond() {
    let (pool, _dir) = fresh_pool();
    // 「前缀 + 毫秒时钟」这种铸 id 的写法，同一毫秒内两次创建会撞主键。
    // 这一条钉住的就是那个失败形态。
    let ids = db::with_conn(&pool, |c| {
        let a = spec::resolve_module(c, "p_demo", "甲", IfExists::Reuse).unwrap();
        let b = spec::resolve_module(c, "p_demo", "乙", IfExists::Reuse).unwrap();
        Ok((a.id, b.id))
    })
    .unwrap();
    assert_ne!(ids.0, ids.1);
}

#[test]
fn resolve_module_rejects_an_empty_name() {
    let (pool, _dir) = fresh_pool();
    let e = db::with_conn(&pool, |c| {
        spec::resolve_module(c, "p_demo", "  ", IfExists::Reuse)
    })
    .unwrap_err();
    assert!(e.to_string().contains("模块名"), "{e}");
}

// =========================================================================
// 集合与路径
// =========================================================================

#[test]
fn resolve_collection_path_walks_into_a_nested_collection() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // fixture 里 m_order 下有「订单 CRUD」，其下有「退款」。
        let path = vec!["订单 CRUD".to_string(), "退款".to_string()];
        let rs = spec::resolve_collection_path(c, "m_order", &path, IfExists::Reuse).unwrap();
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].id, "c_order_crud");
        assert_eq!(rs[1].id, "c_order_refund");
        assert!(rs.iter().all(|r| !r.created));
        Ok(())
    })
    .unwrap();
}

#[test]
fn resolve_collection_path_builds_only_the_missing_levels() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // 「订单 CRUD」已存在，「对账」不存在。
        let path = vec!["订单 CRUD".to_string(), "对账".to_string()];
        let rs = spec::resolve_collection_path(c, "m_order", &path, IfExists::Reuse).unwrap();
        assert!(!rs[0].created, "已存在的那一级不该重建");
        assert!(rs[1].created, "缺的那一级要建出来");
        let child = db::nodes::get(c, &rs[1].id).unwrap();
        assert_eq!(child.parent_id.as_deref(), Some("c_order_crud"));
        assert_eq!(child.root_id, "p_demo");
        Ok(())
    })
    .unwrap();
}

#[test]
fn resolve_collection_path_is_idempotent() {
    let (pool, _dir) = fresh_pool();
    let path = vec!["营销".to_string(), "活动".to_string()];
    let first = db::with_conn(&pool, |c| {
        spec::resolve_collection_path(c, "m_order", &path, IfExists::Reuse)
            .map(|rs| rs.iter().map(|r| r.id.clone()).collect::<Vec<_>>())
    })
    .unwrap();
    let second = db::with_conn(&pool, |c| {
        spec::resolve_collection_path(c, "m_order", &path, IfExists::Reuse)
            .map(|rs| rs.iter().map(|r| r.id.clone()).collect::<Vec<_>>())
    })
    .unwrap();
    assert_eq!(first, second);
}

#[test]
fn resolve_collection_path_rejects_an_empty_path() {
    let (pool, _dir) = fresh_pool();
    let e = db::with_conn(&pool, |c| {
        spec::resolve_collection_path(c, "m_order", &[], IfExists::Reuse)
    })
    .unwrap_err();
    assert!(e.to_string().contains("路径"), "{e}");
}

// =========================================================================
// 路径切分
// =========================================================================

#[test]
fn split_path_cuts_on_slashes_and_trims_each_segment() {
    let v = spec::split_path(" 订单 / 明细 ").unwrap();
    assert_eq!(v, vec!["订单".to_string(), "明细".to_string()]);
}

#[test]
fn split_path_rejects_an_empty_segment() {
    let e = spec::split_path("订单//明细").unwrap_err();
    assert!(e.to_string().contains("空的一段"), "{e}");
}

// =========================================================================
// 端点身份
// =========================================================================

#[test]
fn find_endpoint_by_identity_matches_on_name_method_and_url() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // fixture 里 c_order_crud 下有 POST /orders 与 GET /orders。
        let post =
            spec::find_endpoint_by_identity(c, "c_order_crud", "创建订单", "POST", "/orders")
                .unwrap();
        assert_eq!(post.as_deref(), Some("e_order_create"));
        let get = spec::find_endpoint_by_identity(c, "c_order_crud", "订单列表", "GET", "/orders")
            .unwrap();
        assert_eq!(get.as_deref(), Some("e_order_list"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn find_endpoint_by_identity_ignores_the_verb_case() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        let r = spec::find_endpoint_by_identity(c, "c_order_crud", "创建订单", "post", "/orders")
            .unwrap();
        assert_eq!(r.as_deref(), Some("e_order_create"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn find_endpoint_by_identity_treats_a_different_name_as_a_different_endpoint() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // 同 method 同 url 但名字不同 —— 两个不同的接口，不该被认成一个。
        let r = spec::find_endpoint_by_identity(c, "c_order_crud", "另一个名字", "POST", "/orders")
            .unwrap();
        assert!(r.is_none(), "{r:?}");
        Ok(())
    })
    .unwrap();
}

#[test]
fn find_endpoint_by_identity_does_not_match_a_different_url() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        let r =
            spec::find_endpoint_by_identity(c, "c_order_crud", "创建订单", "POST", "/orders/{id}")
                .unwrap();
        assert!(r.is_none(), "url 不同就不是同一个端点：{r:?}");
        Ok(())
    })
    .unwrap();
}

#[test]
fn find_endpoint_by_identity_does_not_match_across_parents() {
    let (pool, _dir) = fresh_pool();
    db::with_conn(&pool, |c| {
        // 同一个 url 在别的集合下，不该被当成同一个端点。
        let r = spec::find_endpoint_by_identity(c, "c_order_status", "创建订单", "POST", "/orders")
            .unwrap();
        assert!(r.is_none(), "{r:?}");
        Ok(())
    })
    .unwrap();
}

// =========================================================================
// 幂等总账
// =========================================================================

#[test]
fn a_repeated_upsert_never_grows_the_tree() {
    let (pool, _dir) = fresh_pool();
    let count_nodes = |pool: &DbPool| -> usize {
        db::with_conn(pool, |c| {
            Ok(db::nodes::list_by_root(c, "p_demo").unwrap().len())
        })
        .unwrap()
    };
    let before = count_nodes(&pool);

    for _ in 0..3 {
        db::with_conn(&pool, |c| {
            let m = spec::resolve_module(c, "p_demo", "营销", IfExists::Reuse).unwrap();
            let path = vec!["活动".to_string()];
            spec::resolve_collection_path(c, &m.id, &path, IfExists::Reuse)?;
            Ok(())
        })
        .unwrap();
    }

    assert_eq!(
        count_nodes(&pool),
        before + 2,
        "重复三次 upsert 之后，树上只该多出一个模块和一个集合"
    );
}

// =========================================================================
// protocol / graphql 的入参转换
// =========================================================================

#[test]
fn the_protocol_defaults_to_http_and_accepts_the_four_wire_values() {
    use crate::domain::RequestProtocol;
    use crate::spec::input::{parse_protocol, PROTOCOL_VALUES};

    assert_eq!(parse_protocol(None).unwrap(), RequestProtocol::Http);
    assert_eq!(parse_protocol(Some("")).unwrap(), RequestProtocol::Http);
    assert_eq!(parse_protocol(Some("  ")).unwrap(), RequestProtocol::Http);
    for (raw, want) in [
        ("http", RequestProtocol::Http),
        ("graphql", RequestProtocol::GraphQl),
        ("sse", RequestProtocol::Sse),
        ("websocket", RequestProtocol::WebSocket),
        ("GraphQL", RequestProtocol::GraphQl),
    ] {
        assert_eq!(parse_protocol(Some(raw)).unwrap(), want, "{raw}");
    }
    // JSON Schema 里的枚举与解析器接受的值必须来自同一处。
    assert_eq!(
        PROTOCOL_VALUES,
        &["http", "graphql", "sse", "websocket"],
        "PROTOCOL_VALUES 与 RequestProtocol 的线上值一一对应"
    );
    let err = parse_protocol(Some("grpc")).unwrap_err();
    assert!(err.to_string().contains("grpc"), "{err}");
}

#[test]
fn a_graphql_config_must_name_a_non_blank_query() {
    use crate::spec::input::{coerce_graphql, GraphQlRequestInput};

    assert!(
        coerce_graphql(None).unwrap().is_none(),
        "没提到就什么都不要"
    );
    let input = GraphQlRequestInput {
        query: "  ".into(),
        variables: "{}".into(),
        operation_name: None,
    };
    let err = coerce_graphql(Some(input)).unwrap_err();
    assert!(err.to_string().contains("graphql.query"), "{err}");

    let input = GraphQlRequestInput {
        query: "{ ping }".into(),
        variables: r#"{"id": "{{id}}"}"#.into(),
        operation_name: Some("Ping".into()),
    };
    let got = coerce_graphql(Some(input)).unwrap().unwrap();
    assert_eq!(got.query, "{ ping }");
    // variables 原样保留：其中的 `{{id}}` 要等组装时才替换。
    assert_eq!(got.variables, r#"{"id": "{{id}}"}"#);
    assert_eq!(got.operation_name.as_deref(), Some("Ping"));
}
