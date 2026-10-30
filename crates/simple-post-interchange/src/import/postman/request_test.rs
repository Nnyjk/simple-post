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
