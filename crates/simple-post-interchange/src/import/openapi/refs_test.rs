//! `$ref` 展开：本地引用、成环截断与 3.1 的兄弟字段覆盖。

use serde_json::Value;

use crate::diagnostics::Severity;

use super::refs::deref;
use super::support::{first_endpoint, ir_of, warns};
use super::Ctx;

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
fn a_ref_to_an_empty_object_leaves_the_ancestor_frame_on_the_stack() {
    // 访问栈是成环判定的依据。祖先帧的「正在展开」被提前弹掉，那棵子树里真的成环时
    // 就只剩深度上限在起作用，报错会指向错误的方向。
    //
    // 这里直接盯栈：祖先帧压着的指针必须原封不动地留在栈上。
    let root: Value = serde_json::from_str(
        r##"{
          "components": {
            "schemas": {
              "Empty": {},
              "Outer": { "type": "object", "properties": { "inner": { "$ref": "#/components/schemas/Empty" } } }
            }
          }
        }"##,
    )
    .expect("json");

    let mut ctx = Ctx::default();
    ctx.active.push("#/components/schemas/Ancestor".to_string());
    let input = root
        .pointer("/components/schemas/Outer")
        .expect("Outer 在文档里");
    deref(&mut ctx, &root, input, "/outer");

    assert_eq!(
        ctx.active,
        vec!["#/components/schemas/Ancestor".to_string()],
        "祖先帧压进去的指针不该被弹掉：{:?}",
        ctx.active
    );
}

#[test]
fn a_cycle_after_an_empty_ref_is_reported_as_a_cycle_not_as_a_depth_limit() {
    // 「trailingEmpty」先被展开（`Empty` 是空对象，落进标量分支），「self」后被展开。
    // 前者若把祖先帧弹掉，后者的真成环就认不出来，只能一路套到深度上限。
    //
    // 属性名决定顺序：键按字典序入栈，工作栈是后进先出，故**排在后面的先展开**。
    let ex = ir_of(
        r##"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "schemas": {
              "Empty": {},
              "Node": {
                "type": "object",
                "properties": {
                  "self": { "$ref": "#/components/schemas/Node" },
                  "trailingEmpty": { "$ref": "#/components/schemas/Empty" }
                }
              }
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
    assert!(
        ex.diagnostics.iter().any(|d| d.message.contains("成环")),
        "该报成环：{:?}",
        ex.diagnostics
    );
    assert!(
        !ex.diagnostics
            .iter()
            .any(|d| d.message.contains("嵌套太深")),
        "不该报成嵌套太深：{:?}",
        ex.diagnostics
    );
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
