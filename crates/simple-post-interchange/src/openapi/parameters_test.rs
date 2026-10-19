//! 路径参数声明与 operationId 的写法。

use serde_json::Value;
use simple_post_core::domain::{HttpMethod, KeyValue};

use super::support::{doc_of, ep, one_module};
use crate::ir::IrEndpoint;

// -------------------------------------------------------------------------
// 路径参数与 operationId —— 两条都是 Redocly 抓出来的（`path-parameters-defined`
// 报过 4 处、`operation-operationId` 报过 17 处）。linter 是 CI 里的一道门，
// 但那道门只在夹具那一份树上生效，故这两条也要各自钉住。
// -------------------------------------------------------------------------

fn param_of<'a>(doc: &'a Value, path: &str, verb: &str, name: &str) -> Option<&'a Value> {
    doc["paths"][path][verb]["parameters"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name)
}

#[test]
fn a_path_placeholder_is_declared_as_a_path_parameter() {
    // `/users/{id}` 的 `id` 写成 `in: query` 是最常见的错法：工具会认为 URL 里那个
    // `{id}` 无人认领，生成的客户端也就不会把它当成路径参数。
    let ep = IrEndpoint {
        params: vec![KeyValue {
            id: "k".into(),
            key: "id".into(),
            value: "{{id}}".into(),
            enabled: true,
            description: None,
        }],
        ..ep("单个用户", HttpMethod::Get, "/users/{id}")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    let p = param_of(&doc, "/users/{id}", "get", "id").expect("没有声明 {id}");
    assert_eq!(p["in"], "path");
    assert_eq!(p["required"], true);
}

#[test]
fn a_placeholder_without_a_matching_param_row_is_still_declared() {
    // 用户删了参数行但 URL 里占位符还在。缺声明的是**这份文档**，不是用户的配置——
    // 声明出来，工具才知道要把 `{id}` 填进去。
    let doc = doc_of(&one_module(
        "m",
        vec![ep("单个用户", HttpMethod::Get, "/users/{id}")],
    ));

    let p = param_of(&doc, "/users/{id}", "get", "id").expect("占位符必须被声明");
    assert_eq!(p["in"], "path");
}

#[test]
fn a_query_parameter_stays_a_query_parameter() {
    let ep = IrEndpoint {
        params: vec![
            KeyValue {
                id: "k1".into(),
                key: "page".into(),
                value: "1".into(),
                enabled: true,
                description: None,
            },
            KeyValue {
                id: "k2".into(),
                key: "id".into(),
                value: "7".into(),
                enabled: true,
                description: None,
            },
        ],
        ..ep("用户", HttpMethod::Get, "/users/{id}")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    assert_eq!(
        param_of(&doc, "/users/{id}", "get", "page").expect("page")["in"],
        "query"
    );
    assert_eq!(
        param_of(&doc, "/users/{id}", "get", "id").expect("id")["in"],
        "path"
    );
}

#[test]
fn a_disabled_parameter_is_not_declared_at_all() {
    // 禁用的参数不该出现在文档里——它连请求都不会带。
    let ep = IrEndpoint {
        params: vec![KeyValue {
            id: "k".into(),
            key: "trace".into(),
            value: "1".into(),
            enabled: false,
            description: None,
        }],
        ..ep("用户列表", HttpMethod::Get, "/users")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    assert!(param_of(&doc, "/users", "get", "trace").is_none());
}

#[test]
fn operation_id_is_the_verb_plus_the_path_with_identifier_safe_characters() {
    let doc = doc_of(&one_module(
        "m",
        vec![
            ep("单个用户", HttpMethod::Get, "/users/{id}"),
            ep("替换用户", HttpMethod::Put, "/users/{id}"),
        ],
    ));

    // 取自路径而不是接口名：改名是用户随手做的事，而 operationId 是代码生成器拿来
    // 给方法取名的。
    assert_eq!(
        doc["paths"]["/users/{id}"]["get"]["operationId"],
        "get__users__id_"
    );
    assert_eq!(
        doc["paths"]["/users/{id}"]["put"]["operationId"],
        "put__users__id_"
    );

    for verb in ["get", "put"] {
        let id = doc["paths"]["/users/{id}"][verb]["operationId"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "operationId 必须是合法标识符，got: {id}"
        );
    }
}
