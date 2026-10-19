//! 导出：文档形状、动词过滤、`servers` 不回退、`securitySchemes` 归并。

use serde_json::{json, Value};
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BodyMode, HttpMethod, KeyValue, RequestBody,
};
use simple_post_core::error::AppError;

use super::{to_document, DocFormat};
use crate::diagnostics::Severity;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrServer};

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
    // 没有 auth 时写**空的** security 数组，不是不写。
    //
    // 缺席与 `[]` 在 OpenAPI 语义上等价（都不要求凭据），但缺席要读者自己推断一遍；
    // 而 `security-defined` 这类规则只认显式的那一种。写成 `[]` 还让「这条不要求
    // 凭据」变成一句陈述，而不是留给工具去猜。
    assert_eq!(
        doc["paths"]["/e"]["get"]["security"],
        json!([]),
        "没有 auth 的接口要显式声明不要求凭据"
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

/// 一棵带集合的树：模块下两个直接接口，加两个顶层集合，其中一个还套了一层。
fn tree_with_collections() -> Exchange {
    Exchange {
        project_name: "demo".into(),
        modules: vec![IrModule {
            name: "用户".into(),
            endpoints: vec![ep("直接挂在模块上", HttpMethod::Get, "/direct")],
            collections: vec![
                IrCollection {
                    name: "订单".into(),
                    endpoints: vec![ep("订单列表", HttpMethod::Get, "/orders")],
                    collections: vec![IrCollection {
                        name: "明细".into(),
                        endpoints: vec![ep("订单明细", HttpMethod::Get, "/orders/1")],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                IrCollection {
                    name: "退款".into(),
                    endpoints: vec![ep("退款列表", HttpMethod::Get, "/refunds")],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn tags_of<'a>(doc: &'a Value, path: &str) -> Option<&'a Vec<Value>> {
    doc["paths"][path]["get"]["tags"].as_array()
}

#[test]
fn a_collection_name_becomes_the_operation_tag() {
    // tag 是 OpenAPI 里唯一的分组机制。不写它，集合这一层在产物里彻底消失，
    // 重新导入时这些接口全部塌进一个分组。
    let doc = doc_of(&tree_with_collections());
    assert_eq!(
        tags_of(&doc, "/orders"),
        Some(&vec![json!("订单")]),
        "集合里的接口带上集合名"
    );
    assert_eq!(tags_of(&doc, "/refunds"), Some(&vec![json!("退款")]));
    assert!(
        tags_of(&doc, "/direct").is_none(),
        "直接挂在模块上的接口没有集合可依，不编一个 tag"
    );
    // 用过的 tag 都要在根级声明出来。
    assert_eq!(
        doc["tags"],
        json!([{ "name": "订单" }, { "name": "退款" }]),
        "根级 tags 列出用过的每一个"
    );
}

#[test]
fn a_nested_collection_inherits_the_outermost_name() {
    // tag 是扁平字符串，集合的嵌套没有对应构造，只能落一层。取**最外层**：两个不同的
    // 顶层集合下面各有一个同名的子集合时，取内层会让它们在重新导入时并成一处。
    let doc = doc_of(&tree_with_collections());
    assert_eq!(
        tags_of(&doc, "/orders/1"),
        Some(&vec![json!("订单")]),
        "内层集合不覆盖外层的名字"
    );
}

#[test]
fn an_endpoint_that_carries_its_own_tags_keeps_them() {
    // 用户填的分类与树结构无关，不该被集合名顶掉——顶掉等于每次导出都改写一次他的分类。
    let mut ex = tree_with_collections();
    ex.modules[0].collections[0].endpoints[0].tags = vec!["我自己填的".into()];
    let doc = doc_of(&ex);
    assert_eq!(
        tags_of(&doc, "/orders"),
        Some(&vec![json!("我自己填的")]),
        "端点自己的 tags 优先于集合名"
    );
    assert_eq!(
        tags_of(&doc, "/refunds"),
        Some(&vec![json!("退款")]),
        "没有自己 tags 的那个仍走集合名"
    );
}

#[test]
fn a_blank_collection_name_does_not_become_an_empty_tag() {
    // 空名字的 tag 在 Swagger 之类工具里会渲染出一个没有标题的分组，比不分组更糟。
    let mut ex = tree_with_collections();
    ex.modules[0].collections[1].name = "   ".into();
    let doc = doc_of(&ex);
    assert!(
        tags_of(&doc, "/refunds").is_none(),
        "空白集合名不产出 tag: {}",
        doc["paths"]["/refunds"]["get"]
    );
}
