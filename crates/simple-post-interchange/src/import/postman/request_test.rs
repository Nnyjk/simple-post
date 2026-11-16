//! URL 三段落法：path / params / 一台 server，以及拆不出 host 时的降级。

use serde_json::json;

use crate::diagnostics::Severity;

use super::support::{collection_fixture, parse_ok};

#[test]
fn url_splits_into_path_params_and_the_module_cell() {
    let parsed = parse_ok(&collection_fixture());
    let ep = &parsed.exchange.modules[0].collections[0].collections[0].endpoints[0];
    // raw 里的 `?` 之后归 params，host 归模块那一格，path 里两者都不该剩。
    assert_eq!(ep.path, "/orders/{{order_id}}");
    assert!(!ep.path.contains("?"));
    assert!(!ep.path.contains("api.example.com"));
    assert_eq!(ep.params.len(), 1);
    assert_eq!(ep.params[0].value, "true");
    assert_eq!(
        parsed.exchange.modules[0]
            .base_url
            .as_ref()
            .expect("模块那一格")
            .url,
        "https://api.example.com"
    );

    // 没有 raw 时由 protocol/host/path 拼回，同样要拆出 host。
    let no_raw = json!({
        "info": { "name": "n" },
        "item": [{
            "name": "x",
            "request": {
                "method": "GET",
                "url": {
                    "protocol": "http",
                    "host": ["localhost", "8080"],
                    "path": ["ping"],
                    "query": [{ "key": "a", "value": "1" }, { "key": "b", "value": "2", "disabled": true }]
                }
            }
        }]
    });
    let parsed = parse_ok(&no_raw);
    let ep = &parsed.exchange.modules[0].endpoints[0];
    assert_eq!(ep.path, "/ping");
    // 端口留在地址里。
    assert_eq!(
        parsed.exchange.modules[0]
            .base_url
            .as_ref()
            .expect("模块那一格")
            .url,
        "http://localhost:8080"
    );
    assert_eq!(ep.params.len(), 2);
    assert!(ep.params[0].enabled);
    assert!(!ep.params[1].enabled);

    // host 是整体占位：拆不出地址，整串留在 path 里，并出诊断。
    let placeholder = json!({
        "info": { "name": "n" },
        "item": [{
            "name": "x",
            "request": { "method": "GET", "url": "{{baseUrl}}/users" }
        }]
    });
    let parsed = parse_ok(&placeholder);
    let ex = &parsed.exchange;
    assert_eq!(ex.modules[0].endpoints[0].path, "{{baseUrl}}/users");
    assert!(ex.modules[0].base_url.is_none());
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(ex.diagnostics[0].pointer, "/item/0/request/url");
}

/// 一份只有一条请求、响应用例由调用方给的集合。
fn collection_with_responses(responses: serde_json::Value) -> serde_json::Value {
    json!({
        "info": {
            "name": "演示",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [{
            "name": "ping",
            "request": {
                "method": "GET",
                "url": { "raw": "https://api.example.com/ping", "path": ["ping"] }
            },
            "response": responses
        }]
    })
}

#[test]
fn saved_examples_become_response_definitions() {
    let parsed = parse_ok(&collection_with_responses(json!([{
        "name": "OK",
        "code": 200,
        "status": "OK",
        "header": [{ "key": "Content-Type", "value": "application/json; charset=utf-8" }],
        "body": "{\"id\":1}"
    }])));
    let ep = &parsed.exchange.modules[0].endpoints[0];
    assert_eq!(ep.responses.len(), 1);
    assert_eq!(ep.responses[0].status, "200");
    assert_eq!(ep.responses[0].description, "OK");
    // 参数属于这份报文，不属于「这个响应是什么类型」。
    assert_eq!(ep.responses[0].content_type, "application/json");
    assert_eq!(ep.responses[0].example.as_deref(), Some("{\"id\":1}"));
    // Postman 没有响应 schema 这个构造，从正文反推就是猜。
    assert!(ep.responses[0].schema.is_none());
}

#[test]
fn a_repeated_code_keeps_one_definition_and_names_the_rest() {
    let parsed = parse_ok(&collection_with_responses(json!([
        { "name": "第一次", "code": 200, "status": "OK" },
        { "name": "第二次", "code": 200, "status": "OK" }
    ])));
    let ep = &parsed.exchange.modules[0].endpoints[0];
    // 状态码重复会让落库校验拒绝整次导入，故导入时就收成一条。
    assert_eq!(ep.responses.len(), 1);
    assert!(
        parsed
            .exchange
            .diagnostics
            .iter()
            .any(|d| { d.severity == Severity::Warn && d.dropped.as_deref() == Some("200") }),
        "丢掉的重复状态码要点名：{:?}",
        parsed.exchange.diagnostics
    );
}

#[test]
fn a_content_type_that_is_not_a_media_type_is_dropped_and_diagnosed() {
    let parsed = parse_ok(&collection_with_responses(json!([{
        "code": 200,
        "status": "OK",
        "header": [{ "key": "Content-Type", "value": "invalid" }]
    }])));
    let ep = &parsed.exchange.modules[0].endpoints[0];
    assert_eq!(ep.responses[0].content_type, "");
    assert!(
        parsed
            .exchange
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warn
                && d.dropped.as_deref() == Some("Content-Type: invalid")),
        "丢掉的值要点名：{:?}",
        parsed.exchange.diagnostics
    );
}
