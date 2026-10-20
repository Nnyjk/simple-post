//! `parameters` 与 `requestBody`：两级合并、cookie 位置、多 media type 与畸形条目。

use simple_post_core::domain::BodyMode;

use crate::diagnostics::Severity;
use crate::spec::MultipartPart;

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

#[test]
fn a_multipart_body_becomes_a_parts_array_not_a_sample_text() {
    // `BodyMode::Multipart` 的 `content` 装的是**分片数组的 JSON 文本**。
    // 装样例原文的话发送路径解析不出数组，得到零分片，样例静默消失。
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/upload": {
              "post": {
                "requestBody": {
                  "content": {
                    "multipart/form-data": { "example": { "file": "x", "note": "hello" } }
                  }
                },
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let body = &first_endpoint(&ex).body;
    assert_eq!(body.mode, BodyMode::Multipart);
    // 逐字节钉住线格式：`filename` / `contentType` 不出现（它们是 `Option`，
    // 写出 `null` 会被两侧的逐字段校验判成非法输入），键按字典序。
    assert_eq!(
        body.content,
        r#"[{"name":"file","value":"x"},{"name":"note","value":"hello"}]"#
    );
    let parts: Vec<MultipartPart> = serde_json::from_str(&body.content).expect("分片数组");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "file");
    assert_eq!(parts[0].value, "x");
    assert!(parts[0].filename.is_none());
}

#[test]
fn a_multipart_body_without_a_sample_falls_back_to_the_schema_fields() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/upload": {
              "post": {
                "requestBody": {
                  "content": {
                    "multipart/form-data": {
                      "schema": {
                        "type": "object",
                        "properties": {
                          "file": { "type": "string", "example": "a.txt" },
                          "note": { "type": "string" }
                        }
                      }
                    }
                  }
                },
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let body = &first_endpoint(&ex).body;
    assert_eq!(body.mode, BodyMode::Multipart);
    let parts: Vec<MultipartPart> = serde_json::from_str(&body.content).expect("分片数组");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "file");
    assert_eq!(parts[0].value, "a.txt");
    assert_eq!(parts[1].name, "note");
    assert_eq!(parts[1].value, "");
}

#[test]
fn a_multipart_body_with_nothing_to_synthesise_is_downgraded_with_a_diagnostic() {
    // 声称自己能发 multipart 却发不出分片，就是「存得对、发得错」。降级要出声。
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/upload": {
              "post": {
                "requestBody": { "content": { "multipart/form-data": { "schema": { "type": "object" } } } },
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let body = &first_endpoint(&ex).body;
    assert_eq!(body.mode, BodyMode::Raw);
    assert!(
        ex.diagnostics
            .iter()
            .any(|d| d.message.contains("multipart")),
        "降级必须说清楚原因：{:?}",
        ex.diagnostics
    );
}
