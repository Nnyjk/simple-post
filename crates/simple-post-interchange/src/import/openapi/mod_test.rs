//! `parse` 的顶层守卫与分组依据，以及动词键与两种文本输入。

use serde_json::json;
use simple_post_core::domain::HttpMethod;

use super::parse;
use super::support::{dropped, first_endpoint, ir_of};
use crate::import::{parse_text, Format};

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
fn each_tag_becomes_one_group_and_the_untagged_ones_share_another() {
    let ex = ir_of(
        r#"{
          "openapi": "3.0.3",
          "info": { "title": "demo" },
          "paths": {
            "/users": {
              "get": { "tags": ["用户"], "responses": { "200": {} } },
              "post": { "tags": ["用户", "写"], "summary": "建用户", "responses": { "200": {} } }
            },
            "/orders": { "get": { "tags": ["订单"], "responses": { "200": {} } } },
            "/ping": { "get": { "responses": { "200": {} } } },
            "/health": { "get": { "responses": { "200": {} } } }
          }
        }"#,
    );
    assert_eq!(
        ex.modules.len(),
        3,
        "两个 tag 加一个无 tag 分组，而不是每个 operation 一个：{:?}",
        ex.modules.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    let group = |name: &str| -> &crate::ir::IrModule {
        ex.modules
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("没有名为 {name} 的分组"))
    };
    // 同一个 tag 下的两个动词是**同一个**分组里的两行。
    assert_eq!(group("用户").endpoints.len(), 2);
    assert_eq!(group("订单").endpoints.len(), 1);
    // 没有 tag 时归到一个以 info.title 命名的分组，不按路径切。
    assert_eq!(group("demo").endpoints.len(), 2);

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
    // tags 全都留在端点上——归组只认第一个，其余的在这里仍然看得见。
    assert_eq!(post.tags, vec!["用户".to_string(), "写".to_string()]);
    assert!(
        dropped(&ex).contains(&"写"),
        "第二个 tag 没有生成分组，必须点名丢掉：{:?}",
        ex.diagnostics
    );
    // 端点名取不到时依次回退：summary → operationId → `METHOD path`
    assert_eq!(ep_of("/health", HttpMethod::Get).name, "GET /health");
    assert_eq!(ex.endpoint_keys().len(), 5);
}

/// 一份**没有 tag** 的大文档只能落成一个分组。
///
/// 这是真实文档的形状（大量 Apifox / SpringDoc 导出的文件一个 tag 都不打）。文档没给
/// tag 时导入器不能按 operation 各建一个分组，否则 87 个接口会变成 87 个同名集合
/// ——用户看到的是一屏重复的名字，看不出这份文档到底是什么。
#[test]
fn a_document_without_any_tag_yields_a_single_group() {
    let paths: Vec<String> = (0..87)
        .map(|i| {
            format!(
                r#""/api/air/home/thing{i}": {{ "get": {{ "responses": {{ "200": {{}} }} }} }}"#
            )
        })
        .collect();
    let ex = ir_of(&format!(
        r#"{{ "openapi": "3.0.3", "info": {{ "title": "南通6.0" }},
             "servers": [ {{ "url": "http://192.168.11.101:27000" }} ],
             "paths": {{ {} }} }}"#,
        paths.join(",")
    ));

    assert_eq!(ex.modules.len(), 1, "没有 tag 就只有一个分组");
    assert_eq!(ex.modules[0].name, "南通6.0", "分组以文档名命名");
    assert_eq!(ex.modules[0].endpoint_count(), 87);
    assert_eq!(ex.endpoint_count(), 87);
    // 分组键指「整份 paths」这一个位置，不是某个 operation。
    assert_eq!(ex.modules[0].key, "/paths");
    assert_eq!(
        ex.modules[0].base_url.as_ref().expect("模块那一格").url,
        "http://192.168.11.101:27000"
    );
    assert!(
        ex.modules[0].collections.is_empty(),
        "没有第二层：文档没有给任何可嵌套的分组"
    );
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
    assert_eq!(from_yaml.modules[0].base_url, from_json.modules[0].base_url);
    assert_eq!(from_yaml.project_name, from_json.project_name);
}
