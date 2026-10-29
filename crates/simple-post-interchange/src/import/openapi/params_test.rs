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

/// 请求体上的 schema 要导入成 `body.schema`。
///
/// 这条是往返保真的前提：导出器把 `body.schema` 写进 `components.schemas` 并让
/// `requestBody` 指过去，导入器若不把它读回来，「导出再导入」就把这份定义丢了。
#[test]
fn a_request_body_schema_is_imported() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "post": {
                "requestBody": {
                  "content": {
                    "application/json": {
                      "schema": { "type": "object", "title": "UserProfile", "properties": { "id": { "type": "string" } } }
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
    let schema = body.schema.as_deref().expect("请求体上的 schema 要导入");
    let parsed: serde_json::Value = serde_json::from_str(schema).expect("导入的 schema 是 JSON");
    assert_eq!(parsed["title"], "UserProfile");
    assert_eq!(parsed["properties"]["id"]["type"], "string");
}

/// 请求体上的 schema 里带**本地 `$ref`** 时，要展开成内联内容。
///
/// `body.schema` 存的是一段自足的文本：导出器把它写进 `components.schemas` 并让
/// `requestBody` 指过去，而那份 components 是导出时自己生成的——存一段带 `$ref`
/// 的文本进去，导出时那份引用指不到任何东西。
#[test]
fn a_local_ref_inside_the_request_body_schema_is_expanded() {
    let ex = ir_of(
        r##"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "schemas": {
              "Id": { "type": "string" },
              "User": { "type": "object", "properties": { "id": { "$ref": "#/components/schemas/Id" } } }
            }
          },
          "paths": {
            "/users": {
              "post": {
                "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/User" } } } },
                "responses": { "200": {} }
              }
            }
          }
        }"##,
    );
    let body = &first_endpoint(&ex).body;
    let schema = body.schema.as_deref().expect("请求体上的 schema 要导入");
    assert!(
        !schema.contains("$ref"),
        "导进来的 schema 必须自足，不能还指着文档里的 components：{schema}"
    );
    let parsed: serde_json::Value = serde_json::from_str(schema).expect("导入的 schema 是 JSON");
    assert_eq!(parsed["properties"]["id"]["type"], "string");
}

/// 不是对象的 schema（布尔型）不导入，但要**说出来**。
///
/// 布尔 schema 不是导出器认的那种（它要写成 `components.schemas` 里的一个对象），
/// 收进来只会在导出时变成一条「不是合法的 JSON Schema 对象」的诊断。
#[test]
fn a_non_object_request_body_schema_is_not_imported_but_is_reported() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "post": {
                "requestBody": { "content": { "application/json": { "schema": true } } },
                "responses": { "200": {} }
              }
            }
          }
        }"#,
    );
    let body = &first_endpoint(&ex).body;
    assert!(body.schema.is_none(), "布尔 schema 不该被导入");
    assert!(
        ex.diagnostics.iter().any(|d| d.message.contains("schema")),
        "没导入要说一声：{:?}",
        ex.diagnostics
    );
}
