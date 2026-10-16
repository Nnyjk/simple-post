//! Postman v2.1 → IR：树、URL 拆分、body 模式、模板变量与畸形输入。
//!
//! 断言的是**线形状**（key 指针、severity、dropped 文本），因为这三样是外部真的会读的东西：
//! 落库侧拿 key 当选择集、UI 拿 severity 分级、用户拿 dropped 回到源文件。

use serde_json::{json, Value};

use crate::ir::{Exchange, IrCollection, IrEndpoint};

use super::{parse, ParsedExchange};

/// 把一棵交换里的端点展平，顺序 = 模块顺序 + 其内声明顺序。
///
/// 顶层散落的请求现在**共用一个模块**（见 `LOOSE_KEY`），所以按 `modules[i].endpoints[0]`
/// 读会反复读到同一个请求。用这个走查器才不会把「形状变了」误读成「用例坏了」。
pub(super) fn endpoints_in_order(ex: &Exchange) -> Vec<&IrEndpoint> {
    let mut out = Vec::new();
    let mut stack: Vec<&IrCollection> = ex.modules.iter().flat_map(|m| &m.collections).collect();
    for m in &ex.modules {
        out.extend(m.endpoints.iter());
    }
    while let Some(c) = stack.pop() {
        out.extend(c.endpoints.iter());
        stack.extend(c.collections.iter().rev());
    }
    out
}

/// 一份真实形状的集合：一个顶层 folder（内含 sub-folder 与请求）、一个直接挂顶层的请求。
pub(super) fn collection_fixture() -> Value {
    json!({
        "info": {
            "name": "订单服务",
            "description": "order api",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [
            {
                "name": "订单模块",
                "item": [
                    {
                        "name": "订单查询",
                        "item": [
                            {
                                "name": "按 ID",
                                "item": [
                                    {
                                        "name": "查单个订单",
                                        "request": {
                                            "method": "GET",
                                            "header": [
                                                { "key": "X-Trace", "value": "on" },
                                                { "key": "X-Off", "value": "1", "disabled": true }
                                            ],
                                            "url": {
                                                "raw": "https://api.example.com/orders/{{order_id}}?verbose=true",
                                                "protocol": "https",
                                                "host": ["api", "example", "com"],
                                                "path": ["orders", "{{order_id}}"],
                                                "query": [
                                                    { "key": "verbose", "value": "true" }
                                                ]
                                            }
                                        }
                                    }
                                ]
                            }
                        ]
                    },
                    {
                        "name": "建单",
                        "request": {
                            "method": "POST",
                            "auth": {
                                "type": "bearer",
                                "bearer": [{ "key": "token", "value": "{{api_token}}" }]
                            },
                            "body": {
                                "mode": "raw",
                                "raw": "{\"sku\":\"A1\"}",
                                "options": { "raw": { "language": "json" } }
                            },
                            "url": {
                                "raw": "https://api.example.com/orders",
                                "protocol": "https",
                                "host": ["api", "example", "com"],
                                "path": ["orders"]
                            }
                        }
                    }
                ]
            },
            {
                "name": "根级健康检查",
                "request": {
                    "method": "GET",
                    "url": "https://api.example.com/healthz"
                }
            }
        ],
        "variable": [
            { "key": "base", "value": "https://api.example.com" }
        ]
    })
}

pub(super) fn parse_ok(v: &Value) -> ParsedExchange {
    parse(v).expect("这份 fixture 必须是合法的 Postman 文档")
}
