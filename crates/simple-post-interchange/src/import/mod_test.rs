//! 格式嗅探与服务命名：认得出两种形状，不把内层同名键当成顶层键，两种 host 形状归一。

use super::{service_name, sniff, Format};

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

// 服务名。两个导入器共用一份，所以这里把两种外来格式的 host 形状一起钉住：
// 少了任何一条，两侧就会各自开始造出不同的服务。

#[test]
fn a_port_is_not_part_of_a_service_name() {
    assert_eq!(service_name("api.example.com:8080"), "api.example.com");
}

#[test]
fn the_scheme_is_stripped_before_the_name_is_derived() {
    // OpenAPI 的 `servers[].url` 带 scheme，Postman 的 `host` 不带。两者归一。
    assert_eq!(service_name("http://api.example.com"), "api.example.com");
    assert_eq!(
        service_name("https://api.example.com:8443/v2"),
        "api.example.com"
    );
}

#[test]
fn a_variable_host_becomes_the_default_service() {
    // Postman 写 `{{var}}`、OpenAPI 写 `{var}`；两者都不是一台真实存在的主机。
    assert_eq!(service_name("{{baseUrl}}"), "default");
    assert_eq!(service_name("https://{host}/v2"), "default");
}

#[test]
fn an_empty_host_becomes_the_default_service() {
    assert_eq!(service_name(""), "default");
    assert_eq!(service_name("https://"), "default");
}

#[test]
fn a_host_carrying_a_path_is_not_a_service_name() {
    // `/v2` 属于路径。名字里带 `/` 说明调用方递错了东西。
    assert_eq!(service_name("api.example.com/v2"), "default");
}
