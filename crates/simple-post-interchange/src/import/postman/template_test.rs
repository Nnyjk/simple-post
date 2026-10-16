//! `{{...}}`：替换链不认的 token 出声，且一份文件里同一个只报一次。

use serde_json::json;

use crate::diagnostics::Severity;

use super::support::{collection_fixture, parse_ok};

#[test]
fn unresolvable_template_tokens_become_diagnostics() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "带怪变量的请求",
            "request": {
                "method": "GET",
                "header": [{ "key": "X-When", "value": "{{$timestamp}}" }],
                "url": "https://api.example.com/{{my-var}}"
            }
        }]
    }));

    let ex = &parsed.exchange;
    let by_dropped = |d: &str| -> &crate::diagnostics::Diagnostic {
        ex.diagnostics
            .iter()
            .find(|x| x.dropped.as_deref() == Some(d))
            .unwrap_or_else(|| panic!("没有关于 {d} 的诊断：{:?}", ex.diagnostics))
    };

    // 连字符：替换器的 `\w+` 不认，发送时会原样留在 URL 里。
    let bad = by_dropped("{{my-var}}");
    assert_eq!(bad.severity, Severity::Warn);
    assert_eq!(bad.pointer, "/item/0/request/url");
    assert!(bad.message.contains("不会被替换"));

    // Postman 动态变量：本仓库没有对应物，丢弃不影响功能。
    let dyn_var = by_dropped("{{$timestamp}}");
    assert_eq!(dyn_var.severity, Severity::Info);
    assert_eq!(dyn_var.pointer, "/item/0/request/header");

    assert_eq!(ex.diagnostics.len(), 2);
    // 合法的 `{{order_id}}` 形状不该有诊断。
    let ok = parse_ok(&collection_fixture());
    assert!(ok.exchange.diagnostics.is_empty());
}

#[test]
fn the_same_bad_token_is_reported_once_per_document() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "a",
                "request": { "method": "GET", "url": "https://api.example.com/{{my-var}}" }
            },
            {
                "name": "b",
                "request": { "method": "GET", "url": "https://api.example.com/{{my-var}}" }
            }
        ]
    }));
    assert_eq!(parsed.exchange.diagnostics.len(), 1);
}
