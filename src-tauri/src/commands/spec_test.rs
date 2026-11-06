//! 一份接口规格的**线契约**：一份键名，两处必须一致。
//!
//! | 边 | 形状由谁决定 |
//! |---|---|
//! | REST 薄壳 | 整个 body 就是那份规格，键名是 `EndpointSpec` 的字段名 |
//! | MCP 工具 | `spec_upsert` 的参数，键名是同一批字段的 snake_case 写法 |
//!
//! 为什么要读源码而不是断言行为：这条不变量坏掉时的形态很难看。
//!
//! * 一个宿主自己正常：它收到的就是那个 body，它落的也是那份规格。
//! * 另一个宿主每次落规格都报「规格反序列化失败」，或者两份规格落进两套编排，而所有
//!   Rust 测试、前端测试与 `npm run check` 全绿。
//!
//! 两个宿主分处两个进程，进程内没有任何东西能碰到它们的差异。故这里断言两处源码里的
//! 形状，而不是手写一份白名单 —— 白名单会随字段增删而腐化。

use serde_json::json;

use crate::spec::upsert::EndpointSpec;

/// REST 路由的源码。
const ROUTE: &str = include_str!("../../../crates/simple-post-server/src/web_api/routes/spec.rs");
/// MCP 工具的源码。
const TOOL: &str = include_str!("../../../crates/simple-post-mcp/src/mcp/project_ops/spec.rs");

/// 两处调用的是**同一个**实现。落库的编排住在 `crate::spec_api`（再往下是
/// `simple_post_mcp::spec::ops` 与 core 的 `spec::upsert`），任一端自己写一遍，
/// 两端就会对同一份规格给出不同的结果。
#[test]
fn both_hosts_call_the_same_implementation() {
    assert!(
        ROUTE.contains("crate::spec_api::upsert"),
        "the REST route must go through `crate::spec_api::upsert`"
    );
    assert!(
        TOOL.contains("crate::spec::ops::upsert"),
        "the MCP tool must go through `crate::spec::ops::upsert`"
    );
}

/// 这些键名必须是**真正的**线上名，而不只是源码里长得像。REST body 发过去必须真的
/// 落到字段上。
///
/// 两种拼写都收（`project_id` 与 `projectId`）：前者是 MCP 那条线的惯例，后者是
/// REST 与桌面那条线通篇用的写法，两者落在**同一个**类型上，故一份规格从一边搬到
/// 另一边不必翻译。
#[test]
fn the_wire_keys_land_on_the_same_fields() {
    let camel: EndpointSpec = serde_json::from_value(json!({
        "projectId": "p1",
        "module": "交易",
        "collectionPath": "订单/明细",
        "environment": {
            "name": "dev",
            "baseUrl": "https://api.example.test",
            "baseUrlProxy": "https://gw.example.test"
        },
        "endpoint": {
            "name": "查订单",
            "method": "GET",
            "url": "/api/orders/{id}",
            "preRequestScript": "pm.environment.set('a', 'b');"
        }
    }))
    .expect("a camelCase body deserializes");
    assert_eq!(camel.project_id, "p1");
    assert_eq!(camel.collection_path.as_deref(), Some("订单/明细"));
    let environment = camel.environment.as_ref().expect("环境那一段");
    assert_eq!(
        environment.base_url.as_deref(),
        Some("https://api.example.test")
    );
    assert_eq!(
        environment.base_url_proxy.as_deref(),
        Some("https://gw.example.test")
    );
    let endpoint = &camel.endpoint;
    assert_eq!(
        endpoint.pre_request_script.as_deref(),
        Some("pm.environment.set('a', 'b');")
    );

    let snake: EndpointSpec = serde_json::from_value(json!({
        "project_id": "p1",
        "module": "交易",
        "collection_path": "订单/明细",
        "endpoint": { "name": "查订单", "method": "GET", "url": "/api/orders/{id}" }
    }))
    .expect("a snake_case body deserializes");
    assert_eq!(snake.project_id, "p1");
    assert_eq!(snake.collection_path.as_deref(), Some("订单/明细"));
}
