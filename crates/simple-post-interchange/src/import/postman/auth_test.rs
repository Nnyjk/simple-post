//! auth 只读选中方案的那一份载荷；位置表达不了时整段丢弃并出声。

use serde_json::json;
use simple_post_core::domain::{ApiKeyLocation, AuthType};

use crate::diagnostics::Severity;

use super::support::{endpoints_in_order, parse_ok};

#[test]
fn auth_payloads_are_only_read_for_the_selected_scheme() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "basic",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "basic",
                        "basic": [
                            { "key": "username", "value": "u" },
                            { "key": "password", "value": "p" }
                        ]
                    },
                    "url": "https://api.example.com/a"
                }
            },
            {
                "name": "apikey query",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "apikey",
                        "apikey": [
                            { "key": "key", "value": "X-Key" },
                            { "key": "value", "value": "k-1" },
                            { "key": "in", "value": "query" }
                        ]
                    },
                    "url": "https://api.example.com/b"
                }
            },
            {
                "name": "apikey cookie",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "apikey",
                        "apikey": [{ "key": "in", "value": "cookie" }]
                    },
                    "url": "https://api.example.com/c"
                }
            },
            {
                "name": "noauth",
                "request": {
                    "method": "GET",
                    "auth": { "type": "noauth" },
                    "url": "https://api.example.com/d"
                }
            }
        ]
    }));

    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    assert_eq!(eps.len(), 4);
    let basic = &eps[0].auth;
    assert_eq!(basic.kind, AuthType::Basic);
    assert_eq!(basic.basic.as_ref().unwrap().username, "u");
    assert_eq!(basic.basic.as_ref().unwrap().password, "p");

    let apikey = &eps[1].auth;
    assert_eq!(apikey.kind, AuthType::Apikey);
    let ak = apikey.apikey.as_ref().unwrap();
    assert_eq!(ak.key, "X-Key");
    assert_eq!(ak.value, "k-1");
    assert_eq!(ak.in_location, ApiKeyLocation::Query);

    // cookie 位置表达不了：整段丢弃 + 出声，不猜成 header。
    assert_eq!(eps[2].auth.kind, AuthType::None);
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(
        ex.diagnostics[0].dropped.as_deref(),
        Some("apikey auth（in: cookie）")
    );

    assert_eq!(eps[3].auth.kind, AuthType::None);
}
