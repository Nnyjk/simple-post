//! `parameters` 与 `requestBody`：两级合并、cookie 位置、多 media type 与畸形条目。

use simple_post_core::domain::BodyMode;

use crate::diagnostics::Severity;

use super::support::{dropped, first_endpoint, ir_of};

#[test]
fn one_media_type_wins_and_the_others_are_named() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "post": {
                "requestBody": {
                  "content": {
                    "application/xml": { "schema": { "type": "object" } },
                    "application/json": { "example": { "name": "N" } }
                  }
                },
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let body = &first_endpoint(&ex).body;
    assert_eq!(body.mode, BodyMode::Json);
    assert_eq!(body.content, r#"{"name":"N"}"#);
    assert!(
        dropped(&ex).contains(&"application/xml"),
        "被丢掉的 media type 必须点名：{:?}",
        ex.diagnostics
    );
}

#[test]
fn a_cookie_parameter_is_reported_because_the_location_has_no_variant() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "get": {
                "parameters": [
                  { "name": "sid", "in": "cookie", "schema": { "type": "string" } },
                  { "name": "X-Trace", "in": "header", "schema": { "type": "string" } }
                ],
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let ep = first_endpoint(&ex);
    assert!(ep.params.is_empty());
    assert_eq!(ep.headers[0].key, "X-Trace");
    assert!(dropped(&ex).contains(&"cookie sid"));
}

#[test]
fn operation_level_parameters_override_the_path_item_ones() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "parameters": [ { "name": "page", "in": "query", "required": true } ],
              "get": {
                "parameters": [
                  { "name": "page", "in": "query", "required": false, "schema": { "default": 9 } },
                  { "name": "q", "in": "query" }
                ],
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let params = &first_endpoint(&ex).params;
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].key, "page");
    assert_eq!(params[0].value, "9", "operation 级覆盖 path-item 级");
    assert!(!params[0].enabled);
    assert!(params[1].enabled);
}

#[test]
fn a_broken_operation_is_skipped_and_the_rest_still_import() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/ok": { "get": { "responses": { "200": {} } } },
            "/broken": { "get": "不是对象" },
            "/x-thing": { "x-vendor": { "note": "扩展键不算 operation" } }
          }
        }"#,
    );
    assert_eq!(ex.endpoint_count(), 1, "只有那一条能用的操作进来");
    assert_eq!(ex.diagnostics_of(Severity::Error).len(), 1);
}
