//! 格式嗅探：认得出两种形状，且不会把内层同名键当成顶层键。

use super::{sniff, Format};

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
