//! 导入：`$ref` 展开、`servers` 拆分、多 media type、cookie/trace 与两种文本输入。

use serde_json::{json, Value};
use simple_post_core::domain::{BodyMode, HttpMethod};

use super::parse;
use crate::diagnostics::{Diagnostic, Severity};
use crate::import::{parse_text, Format};
use crate::ir::Exchange;

fn ir_of(src: &str) -> Exchange {
    let value: Value =
        serde_json::from_str(src).unwrap_or_else(|e| panic!("测试文档不是 JSON：{e}"));
    parse(&value)
        .unwrap_or_else(|e| panic!("解析失败：{e}"))
        .exchange
}

fn warns(ex: &Exchange) -> Vec<&Diagnostic> {
    ex.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect()
}

fn dropped(ex: &Exchange) -> Vec<&str> {
    ex.diagnostics
        .iter()
        .filter_map(|d| d.dropped.as_deref())
        .collect()
}

fn first_endpoint(ex: &Exchange) -> &crate::ir::IrEndpoint {
    &ex.modules[0].endpoints[0]
}

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
fn the_server_url_splits_into_a_host_and_a_path_prefix() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "servers": [ { "url": "https://api.example.com/v2" } ],
          "paths": { "/users": { "get": { "responses": { "200": {} } } } }
        }"#,
    );
    let ep = first_endpoint(&ex);
    assert_eq!(ep.path, "/v2/users", "url 字段只存 path，host 不进去");
    assert_eq!(ex.modules[0].servers.len(), 1);
    assert_eq!(ex.modules[0].servers[0].url, "https://api.example.com");
    assert_eq!(ex.modules[0].servers[0].service, "api.example.com");
}

#[test]
fn a_second_server_entry_is_reported_because_the_model_has_one_url_per_module() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "servers": [
            { "url": "https://a.example.com" },
            { "url": "https://b.example.com" }
          ],
          "paths": { "/users": { "get": { "responses": { "200": {} } } } }
        }"#,
    );
    assert_eq!(ex.modules[0].servers[0].url, "https://a.example.com");
    assert_eq!(dropped(&ex), vec!["https://b.example.com"]);
}

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
fn trace_lands_as_a_custom_method() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": { "/ping": { "trace": { "responses": { "200": {} } } } }
        }"#,
    );
    let method = &first_endpoint(&ex).method;
    assert_eq!(
        method,
        &HttpMethod::Custom("TRACE".into()),
        "3.0 有 trace 键但 HttpMethod 没有该变体"
    );
    assert_eq!(method.as_str(), "TRACE");
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

#[test]
fn yaml_and_json_text_produce_the_same_tree() {
    let json_text = r#"{
      "openapi": "3.0.3",
      "info": { "title": "demo" },
      "servers": [ { "url": "https://api.example.com/v2" } ],
      "paths": {
        "/users": { "get": { "responses": { "200": {} } } },
        "/users/{id}": {
          "delete": { "parameters": [ { "name": "id", "in": "path", "required": true } ], "responses": { "200": {} } }
        }
      }
    }"#;

    let yaml_text = r#"
openapi: 3.0.3
info:
  title: demo
servers:
  - url: https://api.example.com/v2
paths:
  /users:
    get:
      responses:
        "200": {}
  /users/{id}:
    delete:
      parameters:
        - name: id
          in: path
          required: true
      responses:
        "200": {}
"#;

    let from_json = parse_text(Format::OpenApi3, json_text)
        .unwrap_or_else(|e| panic!("JSON 解析失败：{e}"))
        .exchange;
    let from_yaml = parse_text(Format::OpenApi3, yaml_text)
        .unwrap_or_else(|e| panic!("YAML 解析失败：{e}"))
        .exchange;

    assert_eq!(from_json.endpoint_count(), 2);
    assert_eq!(from_yaml.endpoint_count(), from_json.endpoint_count());
    assert_eq!(from_yaml.endpoint_keys(), from_json.endpoint_keys());
    assert_eq!(from_yaml.modules[0].servers, from_json.modules[0].servers);
    assert_eq!(from_yaml.project_name, from_json.project_name);
}

#[test]
fn each_operation_becomes_a_module_named_after_its_first_tag() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "get": { "tags": ["用户"], "responses": { "200": {} } },
              "post": { "tags": ["用户", "写"], "summary": "建用户", "responses": { "200": {} } }
            },
            "/ping": { "get": { "responses": { "200": {} } } },
            "/health": { "get": { "responses": { "200": {} } } }
          }
        }"#,
    );
    assert_eq!(ex.modules.len(), 4, "一个 operation 一个模块");
    // `paths` 是 BTreeMap，按键排序，故这里按路径与动词找而不是按下标。
    let ep_of = |path: &str, method: HttpMethod| -> &crate::ir::IrEndpoint {
        ex.modules
            .iter()
            .flat_map(|m| m.endpoints.iter())
            .find(|e| e.path == path && e.method == method)
            .unwrap_or_else(|| panic!("没有 {method:?} {path} 这个操作"))
    };
    assert_eq!(ep_of("/users", HttpMethod::Get).name, "GET /users");
    let post = ep_of("/users", HttpMethod::Post);
    assert_eq!(post.name, "建用户", "summary 优先于兜底名");
    assert_eq!(post.tags, vec!["用户".to_string(), "写".to_string()]);
    // 同一个路径的两个动词是两个模块，但都按 tags[0] 命名。
    assert_eq!(ex.modules.iter().filter(|m| m.name == "用户").count(), 2);
    // 没有 tag 时回落到 info.title
    let ping_module = ex
        .modules
        .iter()
        .find(|m| m.endpoints[0].path == "/ping")
        .expect("/ping 在");
    assert_eq!(ping_module.name, "demo");
    // 端点名的兜底顺序：summary → operationId → `METHOD path`
    assert_eq!(ep_of("/health", HttpMethod::Get).name, "GET /health");
    assert_eq!(ex.endpoint_keys().len(), 4);
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
fn mappable_security_schemes_become_auth_and_the_rest_are_reported() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "components": {
            "securitySchemes": {
              "bearerAuth": { "type": "http", "scheme": "bearer" },
              "cookieKey": { "type": "apiKey", "name": "sid", "in": "cookie" },
              "oidc": { "type": "openIdConnect", "openIdConnectUrl": "https://example.com/.well-known" }
            }
          },
          "paths": {
            "/a": { "get": { "security": [ { "cookieKey": [] } ], "responses": { "200": {} } } },
            "/b": { "get": { "security": [ { "bearerAuth": [] } ], "responses": { "200": {} } } },
            "/c": { "get": { "security": [ { "oidc": [] } ], "responses": { "200": {} } } }
          }
        }"#,
    );
    let by_path = |p: &str| -> &crate::ir::IrEndpoint {
        ex.modules
            .iter()
            .flat_map(|m| m.endpoints.iter())
            .find(|e| e.path == p)
            .unwrap_or_else(|| panic!("没有 {p}"))
    };
    assert_eq!(
        by_path("/a").auth.kind,
        simple_post_core::domain::AuthType::None,
        "cookie apiKey 映射不了，端点上就没有 auth"
    );
    assert_eq!(
        by_path("/b").auth.kind,
        simple_post_core::domain::AuthType::Bearer
    );
    assert_eq!(
        by_path("/c").auth.kind,
        simple_post_core::domain::AuthType::None
    );
    assert_eq!(warns(&ex).len(), 2, "cookie 与 openIdConnect 各一条");
}

#[test]
fn a_document_without_usable_paths_is_rejected_outright() {
    let value = json!({ "openapi": "3.0.3", "info": { "title": "demo" }, "paths": [] });
    let err = parse(&value).expect_err("paths 不是对象时整份文档不可用");
    assert!(
        err.to_string().contains("paths"),
        "错误消息要说得清是哪里：{err}"
    );
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
