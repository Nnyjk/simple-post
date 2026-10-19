//! 导出：文档形状、动词过滤、`servers` 不回退、`securitySchemes` 归并。

use serde_json::{json, Value};
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BodyMode, HttpMethod, RequestBody,
};
use simple_post_core::error::AppError;

use super::{to_document, DocFormat};
use crate::diagnostics::Severity;
use crate::ir::{Exchange, IrEndpoint, IrModule, IrServer};

fn ep(name: &str, method: HttpMethod, path: &str) -> IrEndpoint {
    IrEndpoint::new(name, method, path)
}

fn module(name: &str, endpoints: Vec<IrEndpoint>) -> IrModule {
    IrModule {
        name: name.into(),
        endpoints,
        ..Default::default()
    }
}

fn one_module(name: &str, endpoints: Vec<IrEndpoint>) -> Exchange {
    Exchange {
        project_name: "demo".into(),
        modules: vec![module(name, endpoints)],
        ..Default::default()
    }
}

fn doc_of(ex: &Exchange) -> Value {
    let out = to_document(ex, DocFormat::Json).expect("导出应成功");
    serde_json::from_str(&out.content).expect("产物应是合法 JSON")
}

#[test]
fn every_operation_gets_exactly_one_synthetic_200() {
    let ex = one_module(
        "m",
        vec![
            ep("a", HttpMethod::Get, "/a"),
            ep("b", HttpMethod::Post, "/b"),
        ],
    );
    let doc = doc_of(&ex);
    let paths = doc["paths"].as_object().expect("paths 是对象");
    assert_eq!(paths.len(), 2);
    for (_, item) in paths {
        let responses = item["get"]
            .as_object()
            .or_else(|| item["post"].as_object())
            .expect("path item 下有动词");
        let responses = &responses["responses"];
        let keys: Vec<&String> = responses
            .as_object()
            .expect("responses 是对象")
            .keys()
            .collect();
        assert_eq!(keys, vec!["200"], "每个 operation 有且仅有 200");
        assert_eq!(responses["200"]["description"], "OK");
        // 不从响应历史反推 body
        assert!(responses["200"].get("content").is_none());
    }
    assert!(doc["components"].get("schemas").is_none());
}

#[test]
fn a_verb_outside_the_fixed_key_set_is_dropped_with_a_warning() {
    let ex = one_module(
        "m",
        vec![
            ep("find", HttpMethod::Custom("PROPFIND".into()), "/files"),
            ep("ok", HttpMethod::Get, "/files"),
            ep("trace", HttpMethod::Custom("TRACE".into()), "/ping"),
        ],
    );
    let out = to_document(&ex, DocFormat::Json).expect("导出应成功");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");

    assert!(doc["paths"]["/files"].get("propfind").is_none());
    assert_eq!(doc["paths"]["/files"]["get"]["summary"], "ok");
    // `trace` 在固定键集里，`TRACE` 是 Custom 但要落
    assert_eq!(doc["paths"]["/ping"]["trace"]["summary"], "trace");

    let warns: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "只有 PROPFIND 该报 Warn：{:?}",
        out.diagnostics
    );
    assert_eq!(warns[0].dropped.as_deref(), Some("PROPFIND /files"));
}

#[test]
fn servers_are_taken_per_module_and_never_borrowed() {
    let ex = Exchange {
        project_name: "demo".into(),
        modules: vec![
            IrModule {
                name: "with".into(),
                servers: vec![IrServer {
                    service: "api".into(),
                    url: "https://api.example.com".into(),
                }],
                endpoints: vec![ep("a", HttpMethod::Get, "/a")],
                ..Default::default()
            },
            IrModule {
                name: "without".into(),
                endpoints: vec![ep("b", HttpMethod::Get, "/b")],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let doc = doc_of(&ex);

    assert_eq!(
        doc["paths"]["/a"]["get"]["servers"][0]["url"],
        "https://api.example.com"
    );
    assert!(
        doc["paths"]["/b"]["get"].get("servers").is_none(),
        "没有配 URL 的模块不得借用别的模块的"
    );
}

#[test]
fn the_four_mappable_auth_kinds_become_security_schemes() {
    let bearer = IrEndpoint {
        auth: AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some("t".into()),
            ..Default::default()
        },
        ..ep("a", HttpMethod::Get, "/a")
    };
    let basic = IrEndpoint {
        auth: AuthConfig {
            kind: AuthType::Basic,
            ..Default::default()
        },
        ..ep("b", HttpMethod::Get, "/b")
    };
    let key_header = IrEndpoint {
        auth: AuthConfig {
            kind: AuthType::Apikey,
            apikey: Some(ApiKeyAuth {
                key: "X-Api-Key".into(),
                value: String::new(),
                in_location: ApiKeyLocation::Header,
            }),
            ..Default::default()
        },
        ..ep("c", HttpMethod::Get, "/c")
    };
    let key_query = IrEndpoint {
        auth: AuthConfig {
            kind: AuthType::Apikey,
            apikey: Some(ApiKeyAuth {
                key: "token".into(),
                value: String::new(),
                in_location: ApiKeyLocation::Query,
            }),
            ..Default::default()
        },
        ..ep("d", HttpMethod::Get, "/d")
    };
    let none = ep("e", HttpMethod::Get, "/e");

    let ex = one_module("m", vec![bearer, basic, key_header, key_query, none]);
    let doc = doc_of(&ex);
    let schemes = &doc["components"]["securitySchemes"];

    assert_eq!(schemes["bearerAuth"]["type"], "http");
    assert_eq!(schemes["bearerAuth"]["scheme"], "bearer");
    assert_eq!(schemes["basicAuth"]["scheme"], "basic");
    assert_eq!(schemes["X-Api-Key"]["type"], "apiKey");
    assert_eq!(schemes["X-Api-Key"]["name"], "X-Api-Key");
    assert_eq!(schemes["X-Api-Key"]["in"], "header");
    assert_eq!(schemes["token"]["in"], "query");

    assert_eq!(
        doc["paths"]["/a"]["get"]["security"][0]["bearerAuth"],
        json!([])
    );
    assert_eq!(
        doc["paths"]["/c"]["get"]["security"][0]["X-Api-Key"],
        json!([])
    );
    assert!(
        doc["paths"]["/e"]["get"].get("security").is_none(),
        "没有 auth 就不写 security"
    );
}

#[test]
fn body_mode_picks_the_matching_media_type() {
    let ex = one_module(
        "m",
        vec![
            IrEndpoint {
                body: RequestBody {
                    mode: BodyMode::Json,
                    content: "{}".into(),
                    schema: Some(r#"{"type":"object"}"#.into()),
                },
                ..ep("a", HttpMethod::Post, "/a")
            },
            IrEndpoint {
                body: RequestBody {
                    mode: BodyMode::Multipart,
                    content: String::new(),
                    schema: None,
                },
                ..ep("b", HttpMethod::Post, "/b")
            },
            ep("c", HttpMethod::Get, "/c"),
        ],
    );
    let doc = doc_of(&ex);

    assert_eq!(
        doc["paths"]["/a"]["post"]["requestBody"]["content"]["application/json"]["schema"]["type"],
        "object"
    );
    assert!(doc["paths"]["/a"]["post"]["requestBody"]["content"]
        .get("multipart/form-data")
        .is_none());
    assert!(doc["paths"]["/b"]["post"]["requestBody"]["content"]
        .get("multipart/form-data")
        .is_some());
    assert!(doc["paths"]["/c"]["get"].get("requestBody").is_none());

    // 手写且合法的 schema 原样透传
    assert_eq!(doc["components"]["schemas"]["Schema1"]["type"], "object");
}

#[test]
fn an_unparseable_body_schema_is_reported_but_never_fails_the_export() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: String::new(),
                schema: Some("{limit: number}".into()),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let out = to_document(&ex, DocFormat::Json).expect("非法 schema 不该让导出失败");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");
    assert!(doc["components"].get("schemas").is_none());
    let infos: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].dropped.as_deref(), Some("{limit: number}"));
}

#[test]
fn a_json_object_shaped_schema_hint_is_passed_through_as_is() {
    // 判据只有「能否解析成 JSON」：`{"limit":"number"}` 这种形状提示也是合法 JSON，
    // 故原样透传。想校验它是不是 JSON Schema 需要求值器，那是另一件事。
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: String::new(),
                schema: Some("{\"limit\":\"number\"}".into()),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let out = to_document(&ex, DocFormat::Json).expect("导出应成功");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");
    assert_eq!(doc["components"]["schemas"]["Schema1"]["limit"], "number");
    assert!(
        out.diagnostics.is_empty(),
        "没有要报的东西：{:?}",
        out.diagnostics
    );
}

#[test]
fn both_formats_round_trip_through_their_own_parser() {
    let ex = one_module(
        "m",
        vec![
            ep("查询用户", HttpMethod::Get, "/users/{id}"),
            IrEndpoint {
                body: RequestBody {
                    mode: BodyMode::Json,
                    content: r#"{"a":1}"#.into(),
                    schema: None,
                },
                tags: vec!["用户".into(), "用户".into()],
                ..ep("建用户", HttpMethod::Post, "/users")
            },
        ],
    );

    let json_doc = to_document(&ex, DocFormat::Json).expect("JSON 导出");
    let from_json: Value = serde_json::from_str(&json_doc.content).expect("JSON 可读回");
    assert_eq!(from_json["openapi"], "3.0.3");
    assert_eq!(from_json["info"]["title"], "demo");

    let yaml_doc = to_document(&ex, DocFormat::Yaml).expect("YAML 导出");
    let from_yaml: Value = serde_norway::from_str(&yaml_doc.content).expect("YAML 可读回");
    assert_eq!(from_yaml, from_json, "两种格式描述的是同一份文档");

    // 标签去重：文档级只留一份，操作级原样保留
    assert_eq!(from_json["tags"].as_array().expect("tags 是数组").len(), 1);
    assert_eq!(
        from_json["paths"]["/users"]["post"]["tags"]
            .as_array()
            .expect("数组")
            .len(),
        2
    );
}

#[test]
fn doc_format_answers_the_orchestration_questions_and_refuses_to_guess() {
    assert_eq!(DocFormat::parse("json").expect("json"), DocFormat::Json);
    assert_eq!(DocFormat::parse(" YAML ").expect("yaml"), DocFormat::Yaml);
    assert_eq!(DocFormat::Json.media_type(), "application/json");
    assert_eq!(DocFormat::Yaml.extension(), ".yaml");

    // 不认识的格式必须报错，不能悄悄落成 JSON。
    assert!(matches!(
        DocFormat::parse("toml"),
        Err(AppError::Validation(_))
    ));
}
