//! 树形状：folder 逐层成集合、顶层散落请求共分组、畸形条目出声但不妨碍其余落库。

use serde_json::json;
use simple_post_core::domain::{AuthType, BodyMode};

use crate::diagnostics::Severity;

use super::support::{collection_fixture, endpoints_in_order, parse_ok};
use super::LOOSE_KEY;

#[test]
fn a_real_collection_lands_as_modules_collections_and_endpoints() {
    let parsed = parse_ok(&collection_fixture());
    let ex = &parsed.exchange;

    assert_eq!(ex.project_name, "订单服务");
    assert_eq!(ex.project_description.as_deref(), Some("order api"));
    assert!(
        ex.diagnostics.is_empty(),
        "干净文档不该有诊断：{:?}",
        ex.diagnostics
    );

    // 顶层每个 **folder** 各成一个模块。
    assert_eq!(ex.modules.len(), 2);
    let m = &ex.modules[0];
    assert_eq!(m.key, "/item/0");
    assert_eq!(m.name, "订单模块");
    assert_eq!(m.collections.len(), 1);
    assert_eq!(m.collections[0].key, "/item/0/item/0");
    assert_eq!(m.collections[0].name, "订单查询");

    // folder 逐层成为集合，请求落进它所在的那一层。
    let sub = &m.collections[0].collections[0];
    assert_eq!(sub.key, "/item/0/item/0/item/0");
    assert_eq!(sub.name, "按 ID");
    let ep = &sub.endpoints[0];
    assert_eq!(ep.key, "/item/0/item/0/item/0/item/0/request");
    assert_eq!(ep.name, "查单个订单");
    assert_eq!(ep.method.as_str(), "GET");
    assert_eq!(ep.path, "/orders/{{order_id}}");
    assert_eq!(ep.headers.len(), 2);
    assert_eq!(ep.headers[0].key, "X-Trace");
    assert!(ep.headers[0].enabled);
    // `disabled` 必须落到 enabled 上，否则用户禁用的头会被发出去。
    assert!(!ep.headers[1].enabled);
    assert_eq!(ep.params.len(), 1);
    assert_eq!(ep.params[0].key, "verbose");
    assert!(ep.params[0].enabled);

    // 同一模块下的第二个请求挂在模块上（endpoint 可以直接挂模块）。
    assert_eq!(m.endpoints.len(), 1);
    let create = &m.endpoints[0];
    assert_eq!(create.key, "/item/0/item/1/request");
    assert_eq!(create.name, "建单");
    assert_eq!(create.method.as_str(), "POST");
    assert_eq!(create.body.mode, BodyMode::Json);
    assert_eq!(create.body.content, "{\"sku\":\"A1\"}");
    assert_eq!(create.auth.kind, AuthType::Bearer);
    assert_eq!(create.auth.bearer.as_deref(), Some("{{api_token}}"));

    // 直接挂在顶层的请求并进**同一个**模块，名字取集合名 —— 不是各自一个集合。
    let root_module = &ex.modules[1];
    assert_eq!(root_module.key, "/item");
    assert_eq!(root_module.name, "订单服务");
    assert_eq!(root_module.endpoints.len(), 1);
    assert_eq!(root_module.endpoints[0].key, "/item/1/request");
    assert_eq!(root_module.endpoints[0].path, "/healthz");

    // 同一 host 在模块内只算一台服务。
    assert_eq!(m.servers.len(), 1);
    assert_eq!(m.servers[0].service, "api.example.com");
    assert_eq!(m.servers[0].url, "https://api.example.com");
    assert_eq!(ex.endpoint_count(), 3);
}

#[test]
fn deep_folder_nesting_does_not_exhaust_the_stack() {
    // 12 层：递归本身能承受，但工作栈是这里的不变量，故探到明显更深。
    const DEPTH: usize = 12;
    let mut node = json!({
        "name": "最底",
        "request": { "method": "GET", "url": "https://api.example.com/deep" }
    });
    for i in (0..DEPTH).rev() {
        node = json!({ "name": format!("层{i}"), "item": [node] });
    }
    let doc = json!({ "info": { "name": "深" }, "item": [node] });

    let parsed = parse_ok(&doc);
    assert_eq!(parsed.exchange.endpoint_count(), 1);

    // 一路走到底，key 逐层拼接：模块是 `/item/0`，每一层多一段 `/item/0`。
    // 请求的 key 比它所在的那一层多一段——那是这个条目自己的位置。
    let mut col = &parsed.exchange.modules[0].collections[0];
    for d in 2..DEPTH {
        assert_eq!(col.key, "/item/0".repeat(d));
        col = &col.collections[0];
    }
    let deepest = "/item/0".repeat(DEPTH);
    assert_eq!(col.key, deepest);
    assert_eq!(col.endpoints[0].key, format!("{deepest}/item/0/request"));
}

#[test]
fn a_malformed_item_is_reported_and_the_rest_still_lands() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            "不是一个对象",
            { "name": "request 不是对象", "request": "GET" },
            { "name": "缺 url", "request": { "method": "GET" } },
            { "name": "既无 request 也无 item" },
            {
                "name": "好的那个",
                "request": { "method": "GET", "url": "https://api.example.com/ok" }
            }
        ]
    }));

    let ex = &parsed.exchange;
    // 四条坏条目各出一条 Error，其余照常。
    assert_eq!(ex.diagnostics.len(), 4);
    assert!(ex.diagnostics.iter().all(|d| d.severity == Severity::Error));
    assert_eq!(ex.modules.len(), 1);
    assert_eq!(ex.modules[0].endpoints.len(), 1);
    assert_eq!(ex.modules[0].endpoints[0].name, "好的那个");
    assert_eq!(ex.endpoint_count(), 1);
}

#[test]
fn an_unnamed_item_falls_back_to_its_url_then_to_placeholder() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            { "request": { "method": "GET", "url": "https://api.example.com/nameless" } },
            { "request": { "method": "GET", "url": "https://api.example.com/x" } },
            {
                "item": [
                    { "name": "x", "request": { "method": "GET", "url": "https://api.example.com/y" } }
                ]
            },
            { "name": "空文件夹", "item": [] }
        ]
    }));
    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    // 两条顶层散落的请求并进同一个模块，模块名取集合名；**接口**自己的名字仍按 raw URL 回退。
    assert_eq!(ex.modules.len(), 2, "两个散落请求并一个，再加那个文件夹");
    assert_eq!(ex.modules[0].key, LOOSE_KEY);
    assert_eq!(ex.modules[0].name, "n");
    assert_eq!(eps[0].name, "https://api.example.com/nameless");
    assert_eq!(eps[1].name, "https://api.example.com/x");
    // 文件夹没有请求可退，只能叫「未命名」。
    assert_eq!(ex.modules[1].name, "未命名");
    assert_eq!(eps[2].name, "x");
    // 空容器不进树：预览里挂一个什么都没有的模块只会让人以为少导了东西。
    assert_eq!(ex.endpoint_count(), 3);
}

/// 顶层散落的请求共用一个分组；folder 仍各成一个。
///
/// 这条与 `import::openapi::tests::each_tag_becomes_one_group_and_the_untagged_ones_share_another`
/// 是同一条纪律的两侧。曾经的形状是「每个顶层 item 一个模块」，于是一份把请求平铺在根上的
/// 集合（Postman 里很常见）导入后变成 N 个各含 1 个接口的集合。
#[test]
fn loose_top_level_requests_share_one_group_while_folders_stay_apart() {
    let parsed = parse_ok(&json!({
        "info": { "name": "订单服务" },
        "item": [
            { "name": "散落一", "request": { "method": "GET", "url": "/a" } },
            {
                "name": "文件夹甲",
                "item": [{ "name": "甲里的", "request": { "method": "GET", "url": "/b" } }]
            },
            { "name": "散落二", "request": { "method": "POST", "url": "/c" } },
            { "name": "文件夹乙", "item": [] }
        ]
    }));
    let ex = &parsed.exchange;

    assert_eq!(ex.modules.len(), 2, "散落的一组 + 文件夹甲；空文件夹不进树");
    assert_eq!(ex.modules[0].key, LOOSE_KEY);
    assert_eq!(ex.modules[0].name, "订单服务");
    assert_eq!(
        ex.modules[0].endpoints.len(),
        2,
        "两条散落请求在同一个分组里"
    );
    assert_eq!(ex.modules[0].endpoints[0].path, "/a");
    assert_eq!(ex.modules[0].endpoints[1].path, "/c");
    // 分组位置取**第一条**散落请求出现的地方，后面的 folder 排在它之后。
    assert_eq!(ex.modules[1].key, "/item/1");
    assert_eq!(ex.modules[1].name, "文件夹甲");
    assert!(
        ex.modules[1].collections.is_empty(),
        "这个 folder 里全是请求，没有更深的层级"
    );
    assert_eq!(ex.modules[1].endpoints.len(), 1);
    assert_eq!(ex.endpoint_count(), 3);
}
