//! `spec_upsert` Tauri 薄壳的**线契约**：一条键名，三处必须一致。
//!
//! | 边 | 形状由谁决定 |
//! |---|---|
//! | Tauri 命令 | 参数名，宏按 `to_lower_camel_case` 取 payload 键（`ArgumentCase::Camel` 是默认） |
//! | REST 薄壳 | 整个 body 就是那份规格，键名是 `EndpointSpec` 的字段名 |
//! | MCP 工具 | `spec_upsert` 的参数，键名是同一批字段的 snake_case 写法 |
//!
//! 为什么要读源码而不是断言行为：这条不变量坏掉时的形态很难看。
//!
//! * 浏览器形态 100% 正常 —— `POST /api/spec/upsert` 收到的就是那个 body。
//! * 桌面形态每次落规格都报 `invalid args … missing required key spec`，而所有 Rust
//!   测试、前端测试与 `npm run check` 全绿。
//!
//! 缺陷的整个可观测面在 IPC 那一侧，进程内没有任何东西能碰到它。故这里断言三处
//! 源码里的形状，而不是手写一份白名单 —— 白名单会随字段增删而腐化。

use serde_json::json;

use crate::spec::upsert::EndpointSpec;

/// 命令的源码。
const CMDS: &str = include_str!("spec.rs");
/// REST 路由的源码。
const ROUTE: &str = include_str!("../../../crates/simple-post-server/src/web_api/routes/spec.rs");
/// MCP 工具的源码。
const TOOL: &str = include_str!("../../../crates/simple-post-mcp/src/mcp/project_ops/spec.rs");

/// 命令的参数表：`pub fn spec_upsert(` 与收口的那个 `)` 之间。
fn command_params(src: &str, name: &str) -> String {
    let sig = format!("fn {name}(");
    let start = src
        .find(&sig)
        .unwrap_or_else(|| panic!("`{name}` must stay a whole function in spec.rs"))
        + sig.len();
    let rest = &src[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("`{name}` parameter list must close"));
    rest[..end].to_string()
}

/// 命令只收一个 payload 参数，且它叫 `spec` —— 宏按参数名取键，故这个键就是线上键。
#[test]
fn the_command_binds_the_spec_under_one_key() {
    let params = command_params(CMDS, "spec_upsert");
    assert!(
        params.contains("spec: EndpointSpec"),
        "`spec_upsert` must take the spec as one `spec` parameter, got: {params}"
    );
    assert!(
        !params.contains("project_id"),
        "把规格拆成一个个参数就是这条命令要消灭的那件事：{params}"
    );
}

/// 三处调用的是**同一个**实现。落库的编排住在 `crate::spec_api`（再往下是
/// `simple_post_mcp::spec::ops` 与 core 的 `spec::upsert`），任一端自己写一遍，
/// 两端就会对同一份规格给出不同的结果。
#[test]
fn all_three_hosts_call_the_same_implementation() {
    assert!(
        CMDS.contains("crate::spec_api::upsert"),
        "the Tauri command must go through `crate::spec_api::upsert`"
    );
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
