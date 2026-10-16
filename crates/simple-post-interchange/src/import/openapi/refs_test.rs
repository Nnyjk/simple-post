//! `$ref` 展开：本地引用、成环截断与 3.1 的兄弟字段覆盖。

use crate::diagnostics::Severity;

use super::support::{first_endpoint, ir_of, warns};

#[test]
fn a_local_ref_is_expanded_for_both_parameters_and_schemas() {
    let ex = ir_of(
        r##"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "parameters": { "Page": { "name": "page", "in": "query", "schema": { "type": "integer", "default": 2 } } },
            "schemas": { "Id": { "type": "string" } }
          },
          "paths": {
            "/users": {
              "get": {
                "parameters": [
                  { "$ref": "#/components/parameters/Page" },
                  { "name": "id", "in": "header", "schema": { "$ref": "#/components/schemas/Id" } }
                ],
                "responses": { "200": { "description": "OK" } }
              }
            }
          }
        }"##,
    );
    let ep = first_endpoint(&ex);
    assert_eq!(ep.params.len(), 1);
    assert_eq!(ep.params[0].key, "page");
    assert_eq!(ep.params[0].value, "2");
    assert_eq!(ep.headers[0].key, "id");
    assert!(
        ex.diagnostics.is_empty(),
        "正常展开不该有诊断：{:?}",
        ex.diagnostics
    );
}

#[test]
fn a_self_referencing_schema_reports_a_cycle_and_stops() {
    // Node.properties.child 指回 Node 自身。展开必须在这一支停下，而不是
    // 一路套下去把栈跑光。
    let ex = ir_of(
        r##"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "schemas": {
              "Node": { "type": "object", "properties": { "child": { "$ref": "#/components/schemas/Node" } } }
            }
          },
          "paths": {
            "/tree": {
              "get": {
                "parameters": [
                  { "name": "root", "in": "query", "schema": { "$ref": "#/components/schemas/Node" } }
                ],
                "responses": { "200": {} }
              }
            }
          }
        }"##,
    );
    assert_eq!(ex.endpoint_count(), 1);
    let cycles = warns(&ex);
    assert_eq!(cycles.len(), 1, "成环只该报一次：{:?}", ex.diagnostics);
    assert!(cycles[0].message.contains("成环"));
    assert_eq!(
        cycles[0].dropped.as_deref(),
        Some("#/components/schemas/Node")
    );
    // 外层参数本身还在：被截断的是 schema 那一支，不是整个操作。
    assert_eq!(first_endpoint(&ex).params[0].key, "root");
}

#[test]
fn a_31_union_type_and_ref_siblings_are_both_tolerated() {
    let ex = ir_of(
        r##"{
          "openapi": "3.1.0",
          "info": { "title": "demo" },
          "components": { "schemas": { "Id": { "type": "string" } } },
          "paths": {
            "/users": {
              "get": {
                "parameters": [
                  { "name": "nick", "in": "query", "required": false, "schema": { "type": ["string", "null"] } },
                  { "name": "id", "in": "header", "schema": { "$ref": "#/components/schemas/Id", "description": "兄弟字段" } }
                ],
                "responses": { "200": {} }
              }
            }
          }
        }"##,
    );
    let ep = first_endpoint(&ex);
    assert_eq!(ep.params[0].key, "nick");
    assert!(!ep.params[0].enabled, "required: false 即禁用");
    assert_eq!(ep.headers[0].key, "id");
    assert_eq!(ex.diagnostics_of(Severity::Error).len(), 0);
}
