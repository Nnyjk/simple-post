//! 格式嗅探与服务命名：认得出两种形状，不把内层同名键当成顶层键，两种 host 形状归一。

use super::{sniff, sniff_and_parse, Format};

fn ok(src: &str) -> Format {
    sniff(src).unwrap_or_else(|e| panic!("sniff failed: {e}"))
}

#[test]
fn a_postman_collection_is_recognised() {
    let src = r#"{
      "info": { "name": "demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "item": [ { "name": "Get users", "request": { "method": "GET", "url": "/users" } } ]
    }"#;
    assert_eq!(ok(src), Format::PostmanV2_1);
    assert_eq!(Format::PostmanV2_1.as_str(), "postman-collection-v2.1");
}

#[test]
fn an_openapi_document_is_recognised() {
    let src = r#"{
      "openapi": "3.0.3",
      "info": { "title": "demo", "version": "1.0.0" },
      "paths": { "/users": { "get": { "responses": { "200": {} } } } }
    }"#;
    assert_eq!(ok(src), Format::OpenApi3);
    assert_eq!(Format::OpenApi3.as_str(), "openapi-3");
}

#[test]
fn the_yaml_shape_is_recognised_too() {
    // 行首键名路径。此处不经过真正的 YAML 解析器——依赖是 #4 的事。
    let src = "openapi: 3.0.3\ninfo:\n  title: demo\npaths:\n  /users:\n    get: {}\n";
    assert_eq!(ok(src), Format::OpenApi3);
}

#[test]
fn a_nested_item_key_does_not_make_it_a_collection() {
    // 顶层只有 `info`，`item` 在嵌套对象里——不该判成 Postman。
    let src = r#"{ "info": { "title": "demo" }, "x-meta": { "item": [1, 2] } }"#;
    assert!(sniff(src).is_err());
}

#[test]
fn a_compressed_openapi_document_is_recognised() {
    // 单行压缩过的 JSON 没有行首可言：CDN 上的 spec 与 `JSON.stringify` 的产物
    // 都是这个形状，认不出来等于「导入完全不可用」。
    let src = r#"{"openapi":"3.0.3","info":{"title":"demo","version":"1.0.0"},"paths":{"/users":{"get":{"responses":{"200":{}}}}}}"#;
    assert_eq!(ok(src), Format::OpenApi3);
}

#[test]
fn a_compressed_postman_collection_is_recognised() {
    let src = r#"{"info":{"name":"demo","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"Get users","request":{"method":"GET","url":"/users"}}]}"#;
    assert_eq!(ok(src), Format::PostmanV2_1);
}

#[test]
fn a_nested_item_key_does_not_make_a_compressed_document_a_collection() {
    // 顶层有 `openapi`/`paths`，`item` 与 `info` 只在内层。判据必须是顶层键，
    // 退化成「文本里出现过 item 与 info」就会误判成 Postman。
    let src = r#"{"openapi":"3.0.3","x":{"item":{},"info":{}},"paths":{}}"#;
    assert_eq!(ok(src), Format::OpenApi3);
}

#[test]
fn sniffing_and_parsing_agree_with_sniffing_alone() {
    // 两条路必须对同一份文本给出同一个格式：一条给 UI 看，一条落库。
    let src = r#"{"openapi":"3.0.3","info":{"title":"demo","version":"1"},"paths":{"/users":{"get":{"responses":{"200":{}}}}}}"#;
    let (format, parsed) = sniff_and_parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    assert_eq!(format, Format::OpenApi3);
    assert_eq!(parsed.exchange.project_name, "demo");
}

#[test]
fn a_bom_does_not_hide_the_first_key() {
    let src = "\u{feff}{\n  \"info\": {},\n  \"item\": []\n}";
    assert_eq!(ok(src), Format::PostmanV2_1);
}

#[test]
fn unknown_content_is_rejected_with_a_readable_message() {
    let err = sniff("[]").expect_err("an array is neither format");
    assert!(
        err.to_string().contains("认不出"),
        "message should tell the user what we looked for, got: {err}"
    );
}
