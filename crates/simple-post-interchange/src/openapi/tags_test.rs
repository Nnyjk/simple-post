//! 集合 → tag 的投影：最外层集合名、端点自带的 tags 与空名字。

use serde_json::{json, Value};
use simple_post_core::domain::HttpMethod;

use super::support::{doc_of, ep};
use crate::ir::{Exchange, IrCollection, IrModule};

/// 一棵带集合的树：模块下两个直接接口，加两个顶层集合，其中一个还套了一层。
fn tree_with_collections() -> Exchange {
    Exchange {
        project_name: "demo".into(),
        modules: vec![IrModule {
            name: "用户".into(),
            endpoints: vec![ep("直接挂在模块上", HttpMethod::Get, "/direct")],
            collections: vec![
                IrCollection {
                    name: "订单".into(),
                    endpoints: vec![ep("订单列表", HttpMethod::Get, "/orders")],
                    collections: vec![IrCollection {
                        name: "明细".into(),
                        endpoints: vec![ep("订单明细", HttpMethod::Get, "/orders/1")],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                IrCollection {
                    name: "退款".into(),
                    endpoints: vec![ep("退款列表", HttpMethod::Get, "/refunds")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn tags_of<'a>(doc: &'a Value, path: &str) -> Option<&'a Vec<Value>> {
    doc["paths"][path]["get"]["tags"].as_array()
}

#[test]
fn a_collection_name_becomes_the_operation_tag() {
    // tag 是 OpenAPI 里唯一的分组机制。不写它，集合这一层在产物里彻底消失，
    // 重新导入时这些接口全部塌进一个分组。
    let doc = doc_of(&tree_with_collections());
    assert_eq!(
        tags_of(&doc, "/orders"),
        Some(&vec![json!("订单")]),
        "集合里的接口带上集合名"
    );
    assert_eq!(tags_of(&doc, "/refunds"), Some(&vec![json!("退款")]));
    assert!(
        tags_of(&doc, "/direct").is_none(),
        "直接挂在模块上的接口没有集合可依，不编一个 tag"
    );
    // 用过的 tag 都要在根级声明出来。
    assert_eq!(
        doc["tags"],
        json!([{ "name": "订单" }, { "name": "退款" }]),
        "根级 tags 列出用过的每一个"
    );
}

#[test]
fn a_nested_collection_inherits_the_outermost_name() {
    // tag 是扁平字符串，集合的嵌套没有对应构造，只能落一层。取**最外层**：两个不同的
    // 顶层集合下面各有一个同名的子集合时，取内层会让它们在重新导入时并成一处。
    let doc = doc_of(&tree_with_collections());
    assert_eq!(
        tags_of(&doc, "/orders/1"),
        Some(&vec![json!("订单")]),
        "内层集合不覆盖外层的名字"
    );
}

#[test]
fn an_endpoint_that_carries_its_own_tags_keeps_them() {
    // 用户填的分类与树结构无关，不该被集合名顶掉——顶掉等于每次导出都改写一次他的分类。
    let mut ex = tree_with_collections();
    ex.modules[0].collections[0].endpoints[0].tags = vec!["我自己填的".into()];
    let doc = doc_of(&ex);
    assert_eq!(
        tags_of(&doc, "/orders"),
        Some(&vec![json!("我自己填的")]),
        "端点自己的 tags 优先于集合名"
    );
    assert_eq!(
        tags_of(&doc, "/refunds"),
        Some(&vec![json!("退款")]),
        "没有自己 tags 的那个仍走集合名"
    );
}

#[test]
fn a_blank_collection_name_does_not_become_an_empty_tag() {
    // 空名字的 tag 在 Swagger 之类工具里会渲染出一个没有标题的分组，比不分组更糟。
    let mut ex = tree_with_collections();
    ex.modules[0].collections[1].name = "   ".into();
    let doc = doc_of(&ex);
    assert!(
        tags_of(&doc, "/refunds").is_none(),
        "空白集合名不产出 tag: {}",
        doc["paths"]["/refunds"]["get"]
    );
}
