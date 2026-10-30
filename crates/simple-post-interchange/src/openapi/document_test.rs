//! 文档形状、动词过滤、`servers` 不回退、`securitySchemes` 归并与格式选择。

use serde_json::{json, Value};
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BodyMode, HttpMethod, RequestBody,
};
use simple_post_core::error::AppError;

use super::support::{doc_of, ep, one_module};
use super::{to_document, DocFormat};
use crate::diagnostics::Severity;
use crate::ir::{Exchange, IrBaseUrl, IrCollection, IrEndpoint, IrModule, IrScripts};

/// 这个 operation 的 `requestBody` 引用的 component 名字。
///
/// 名字是**内容派生**的（见 `Builder::claim_schema_name`），故测试不去硬编码它，
/// 而是顺着 `$ref` 找——这样测试盯的是「引用指得到」，而不是「哈希算法没变」。
fn referenced_schema_name(doc: &Value, path: &str, verb: &str) -> String {
    let reference = doc["paths"][path][verb]["requestBody"]["content"]["application/json"]
        ["schema"]["$ref"]
        .as_str()
        .expect("requestBody 必须是一个 $ref");
    reference
        .rsplit('/')
        .next()
        .expect("$ref 的末段是 component 名字")
        .to_string()
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

/// Postman 的 `{{name}}` 换写成 OpenAPI 的 `{name}`，占位符照常被声明成 path 参数。
///
/// **不做换写会怎样**：文档里留着双花括号，`paths` 键不合法，而 `path_placeholders`
/// 取第一个 `{` 到其后第一个 `}` 之间，得出 `{userId`（多带一个左括号）——一个对应不
/// 上任何模板变量的参数名。这条会挂在本 crate 给自己建的那道 openapiv3 / Redocly 校验
/// 上，而导出照样报成功、不出任何诊断。
#[test]
fn a_postman_template_in_the_path_becomes_an_openapi_path_parameter() {
    let ex = one_module(
        "m",
        vec![ep("a", HttpMethod::Get, "/users/{{userId}}/detail")],
    );
    let out = to_document(&ex, DocFormat::Json).expect("导出应成功");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");

    let key = "/users/{userId}/detail";
    assert!(
        doc["paths"].get(key).is_some(),
        "换写后的键必须在：{:?}",
        doc["paths"]
    );
    assert!(
        doc["paths"]
            .as_object()
            .unwrap()
            .get("/users/{{userId}}/detail")
            .is_none(),
        "双花括号那个键不能留着：{:?}",
        doc["paths"]
    );

    let params = doc["paths"][key]["get"]["parameters"]
        .as_array()
        .expect("声明了参数");
    assert_eq!(
        params,
        &vec![json!({
            "name": "userId",
            "in": "path",
            "required": true,
            "schema": { "type": "string" },
        })],
        "参数名里不能多一个左括号"
    );
    assert!(
        out.diagnostics.iter().all(|d| d.severity != Severity::Warn),
        "换写之后不该有告警：{:?}",
        out.diagnostics
    );
}

/// 「整个 host 是一个变量」的 path 跳过整条操作，并说明原因。
///
/// 导入时那条 path 本来就因为拆不出稳定服务名而原样留着（`{{baseUrl}}/users`，并出了
/// 一条诊断）。导出时它换不成以 `/` 开头的 OpenAPI 键——**本仓库没有能表达它的形状**。
/// 与其产出一份不合法的文档，不如跳过并出声：这正是本 crate「有损的一侧必须出诊断」
/// 那条规则。
#[test]
fn a_path_that_cannot_become_an_openapi_key_is_skipped_with_a_warning() {
    let ex = one_module(
        "m",
        vec![
            ep("placeholder host", HttpMethod::Get, "{{baseUrl}}/users"),
            ep("fine", HttpMethod::Get, "/users"),
        ],
    );
    let out = to_document(&ex, DocFormat::Json).expect("导出应成功");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");

    assert_eq!(
        doc["paths"].as_object().expect("paths").len(),
        1,
        "只有那条能表达的留下了：{:?}",
        doc["paths"]
    );
    assert!(
        doc["paths"]["/users"]["get"].is_object(),
        "留下的必须是可表达的那条：{:?}",
        doc["paths"]
    );

    let warns: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 1, "{:?}", out.diagnostics);
    assert_eq!(warns[0].dropped.as_deref(), Some("GET {{baseUrl}}/users"));
}

#[test]
fn servers_are_taken_per_module_and_never_borrowed() {
    let ex = Exchange {
        project_name: "demo".into(),
        modules: vec![
            IrModule {
                name: "with".into(),
                base_url: Some(IrBaseUrl {
                    url: "https://api.example.com".into(),
                    proxy: None,
                }),
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

/// 一个带地址的模块，底下挂着一个端点。
fn module_with_cell(cell_url: &str, endpoint: IrEndpoint) -> Exchange {
    Exchange {
        project_name: "demo".into(),
        modules: vec![IrModule {
            name: "m".into(),
            base_url: Some(IrBaseUrl {
                url: cell_url.into(),
                proxy: None,
            }),
            endpoints: vec![endpoint],
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// 端点自己配了地址时，产物写那个地址。
///
/// 端点带自己的地址时不套用模块那一格，故产物必须写端点自己的那个：写模块那一格的
/// 地址，描述的就不是这个端点实际会打的地址。
#[test]
fn an_endpoint_with_its_own_address_writes_that_address_into_servers() {
    let ex = module_with_cell(
        "https://api.example.com",
        IrEndpoint {
            base_url_override: Some("https://own.example.com".into()),
            ..ep("a", HttpMethod::Get, "/a")
        },
    );
    let doc = doc_of(&ex);

    assert_eq!(
        doc["paths"]["/a"]["get"]["servers"],
        json!([{ "url": "https://own.example.com" }]),
        "端点自己的地址优先于模块那一格：{:?}",
        doc["paths"]["/a"]["get"]
    );
}

/// 端点自带完整地址（`base_url_override` 是空串）时没有服务器前缀可写。
///
/// 这一格地址是端点自己带的，模块那一格与它无关，故产物不写 `servers`：写了模块那一格
/// 的地址，描述的就不是这个端点实际会打的地址。
#[test]
fn an_endpoint_that_carries_its_whole_address_writes_no_servers() {
    let ex = module_with_cell(
        "https://api.example.com",
        IrEndpoint {
            base_url_override: Some(String::new()),
            ..ep("a", HttpMethod::Get, "/a")
        },
    );
    let doc = doc_of(&ex);

    assert!(
        doc["paths"]["/a"]["get"].get("servers").is_none(),
        "端点自带完整地址时不该写 servers：{:?}",
        doc["paths"]["/a"]["get"]
    );
}

/// 端点没有自己的地址时取模块那一格。
#[test]
fn an_endpoint_without_its_own_address_writes_the_module_cell_address() {
    let ex = module_with_cell("https://api.example.com", ep("a", HttpMethod::Get, "/a"));
    let doc = doc_of(&ex);

    assert_eq!(
        doc["paths"]["/a"]["get"]["servers"],
        json!([{ "url": "https://api.example.com" }]),
        "端点没有自己的地址时取模块那一格：{:?}",
        doc["paths"]["/a"]["get"]
    );
}

/// 模块那一格没有地址时，产物不写 `servers`。
///
/// 格子缺失与格子里的地址是空串都是「没有地址」这两种情形。写一个空的 `servers` 会让
/// 读产物的人以为这个接口挂在空地址上。
#[test]
fn a_module_cell_without_an_address_writes_no_servers() {
    let ex = module_with_cell("", ep("a", HttpMethod::Get, "/a"));
    let doc = doc_of(&ex);

    assert!(
        doc["paths"]["/a"]["get"].get("servers").is_none(),
        "模块那一格没有地址时不该写 servers：{:?}",
        doc["paths"]["/a"]["get"]
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

    // 带 schema 的那条写 $ref，且那个名字在 components 里真的有一份定义。
    let name = referenced_schema_name(&doc, "/a", "post");
    assert_eq!(
        doc["paths"]["/a"]["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        format!("#/components/schemas/{name}")
    );
    assert!(doc["paths"]["/a"]["post"]["requestBody"]["content"]
        .get("multipart/form-data")
        .is_none());
    assert!(doc["paths"]["/b"]["post"]["requestBody"]["content"]
        .get("multipart/form-data")
        .is_some());
    // 没有 schema 的那条只声明「这是一个 object」——字段不猜。
    assert_eq!(
        doc["paths"]["/b"]["post"]["requestBody"]["content"]["multipart/form-data"]["schema"]
            ["type"],
        "object"
    );
    assert!(doc["paths"]["/c"]["get"].get("requestBody").is_none());

    // 手写且合法的 schema 原样透传
    assert_eq!(doc["components"]["schemas"][&name]["type"], "object");
}

/// 端点带了 `body.schema` 时，`requestBody` 必须**引用**它，而不是另写一个。
///
/// `collect_schema` 把 schema 收进 `components.schemas`，故 `requestBody` 必须生成
/// 对应的 `$ref`。若 `requestBody` 恒写死 `{"type": "object"}`，产物里会躺着一堆没人
/// 引用的定义，而每个 requestBody 都在宣称一个错的请求体形状。
#[test]
fn a_body_schema_is_referenced_from_the_request_body() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: String::new(),
                schema: Some(r#"{"type":"object","properties":{"id":{"type":"string"}}}"#.into()),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let doc = doc_of(&ex);

    let reference =
        &doc["paths"]["/a"]["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"];
    let reference = reference.as_str().expect("requestBody 必须是一个 $ref");
    // `$ref` 要能在文档里解析到。
    let name = reference
        .rsplit('/')
        .next()
        .expect("$ref 的末段是 component 名字");
    assert_eq!(
        doc["components"]["schemas"][name]["properties"]["id"]["type"], "string",
        "$ref 必须解析得到：{reference}"
    );
    assert!(
        doc["paths"]["/a"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            .get("type")
            .is_none(),
        "引用之后不再内联一个形状"
    );
}

/// 写了 `title` 的 schema 按 title 命名。
///
/// `title` 是 JSON Schema 自己的字段：用户在界面的 Schema 编辑区里写它，就是在给
/// 这份定义起名。读文档的人看到 `UserProfile` 立刻知道它是什么，而 `Schema1` 这种
/// 按位置编出来的名字什么也不说。
#[test]
fn a_schema_with_a_title_is_named_after_that_title() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: "{}".into(),
                schema: Some(
                    r#"{"type":"object","title":"UserProfile","properties":{"id":{"type":"string"}}}"#
                        .into(),
                ),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let doc = doc_of(&ex);

    assert_eq!(referenced_schema_name(&doc, "/a", "post"), "UserProfile");
    assert_eq!(
        doc["components"]["schemas"]["UserProfile"]["title"],
        "UserProfile"
    );
}

/// 名字由**内容**决定，不按出现顺序编号。
///
/// 位置编号有两个毛病：文档里出现 `Schema1`、`Schema2`… 这种读不出任何信息的名字；
/// 而删掉一个接口之后，后面每一个的名字都往前挪，同一份文档在两次导出之间对不上号。
///
/// 这条用两份产物对比：同一份定义在其中的位置不同，名字必须一样。
#[test]
fn a_schema_name_does_not_depend_on_its_position() {
    let schema = r#"{"type":"object","properties":{"id":{"type":"string"}}}"#;
    let with_schema = || IrEndpoint {
        body: RequestBody {
            mode: BodyMode::Json,
            content: "{}".into(),
            schema: Some(schema.into()),
        },
        ..ep("a", HttpMethod::Post, "/a")
    };

    let alone = doc_of(&one_module("m", vec![with_schema()]));
    let after_another = doc_of(&one_module(
        "m",
        vec![
            IrEndpoint {
                body: RequestBody {
                    mode: BodyMode::Json,
                    content: "{}".into(),
                    schema: Some(r#"{"type":"object","title":"Other"}"#.into()),
                },
                ..ep("x", HttpMethod::Post, "/x")
            },
            with_schema(),
        ],
    ));

    assert_eq!(
        referenced_schema_name(&alone, "/a", "post"),
        referenced_schema_name(&after_another, "/a", "post"),
        "名字不能随位置变"
    );
}

/// 同一份定义出现在两个接口上时只写一份，两个 operation 都 `$ref` 它。
#[test]
fn the_same_schema_on_two_endpoints_is_written_once() {
    let schema = r#"{"type":"object","title":"UserProfile","properties":{"id":{"type":"string"}}}"#;
    let endpoint = |name: &str, path: &str| IrEndpoint {
        body: RequestBody {
            mode: BodyMode::Json,
            content: "{}".into(),
            schema: Some(schema.into()),
        },
        ..ep(name, HttpMethod::Post, path)
    };
    let ex = one_module("m", vec![endpoint("a", "/a"), endpoint("b", "/b")]);
    let doc = doc_of(&ex);

    let name_a = referenced_schema_name(&doc, "/a", "post");
    let name_b = referenced_schema_name(&doc, "/b", "post");
    assert_eq!(
        name_a, name_b,
        "同一份定义只写一份，两个 operation 指同一个名字"
    );
    assert_eq!(
        doc["components"]["schemas"]
            .as_object()
            .expect("对象")
            .len(),
        1,
        "components.schemas 里只有一份"
    );
}

/// 键序不同的两份内容是**同一份**定义：去重按规范文本（键序无关）比对。
#[test]
fn the_same_schema_written_in_a_different_key_order_is_still_one_component() {
    let endpoint = |path: &str, schema: &str| IrEndpoint {
        body: RequestBody {
            mode: BodyMode::Json,
            content: "{}".into(),
            schema: Some(schema.into()),
        },
        ..ep("e", HttpMethod::Post, path)
    };
    let ex = one_module(
        "m",
        vec![
            endpoint(
                "/a",
                r#"{"type":"object","title":"User","properties":{"id":{"type":"string"}}}"#,
            ),
            endpoint(
                "/b",
                r#"{"properties":{"id":{"type":"string"}},"title":"User","type":"object"}"#,
            ),
        ],
    );
    let doc = doc_of(&ex);

    assert_eq!(
        referenced_schema_name(&doc, "/a", "post"),
        referenced_schema_name(&doc, "/b", "post"),
        "键序不同不该产生两份定义"
    );
    assert_eq!(
        doc["components"]["schemas"]
            .as_object()
            .expect("对象")
            .len(),
        1,
        "components.schemas 里只有一份"
    );
}

/// 两份**内容不同**的定义即使同名（同一个 `title`）也不能共用一个名字。
///
/// 共用名字的后果是后写的那份覆盖前一份：产物里只剩一份定义，而两个 operation
/// 都指着它——读到的是错的形状，比没有更糟。
#[test]
fn two_different_schemas_with_the_same_title_do_not_share_a_name() {
    let endpoint = |path: &str, schema: &str| IrEndpoint {
        body: RequestBody {
            mode: BodyMode::Json,
            content: "{}".into(),
            schema: Some(schema.into()),
        },
        ..ep("e", HttpMethod::Post, path)
    };
    let ex = one_module(
        "m",
        vec![
            endpoint(
                "/a",
                r#"{"type":"object","title":"User","properties":{"a":{"type":"string"}}}"#,
            ),
            endpoint(
                "/b",
                r#"{"type":"object","title":"User","properties":{"b":{"type":"string"}}}"#,
            ),
        ],
    );
    let doc = doc_of(&ex);

    let name_a = referenced_schema_name(&doc, "/a", "post");
    let name_b = referenced_schema_name(&doc, "/b", "post");
    assert_ne!(name_a, name_b, "两份内容不同的定义不能共用一个名字");
    assert_eq!(
        doc["components"]["schemas"][&name_a]["properties"]["a"]["type"], "string",
        "第一份没被覆盖"
    );
    assert_eq!(
        doc["components"]["schemas"][&name_b]["properties"]["b"]["type"], "string",
        "第二份没被覆盖"
    );
}

/// `title` 里的非法字符要换成合法的 component 键字符。
///
/// component 键只允许 `[A-Za-z0-9._-]`，一个带空格的标题直接写进去就是一份不合规的
/// 文档，而 Redocly 与多数代码生成器都会因此报错。
#[test]
fn a_title_with_illegal_characters_becomes_a_legal_component_key() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: "{}".into(),
                schema: Some(r#"{"type":"object","title":"Create User"}"#.into()),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let doc = doc_of(&ex);

    let name = referenced_schema_name(&doc, "/a", "post");
    assert_eq!(name, "Create_User");
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
        "component 键只允许 [A-Za-z0-9._-]，got: {name}"
    );
}

/// 没有 `body.schema` 的端点不得在 `components.schemas` 里留下孤儿定义。
#[test]
fn an_endpoint_without_a_body_schema_leaves_no_orphan_component() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::Json,
                content: "{}".into(),
                schema: None,
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let doc = doc_of(&ex);

    assert!(
        doc["components"].get("schemas").is_none(),
        "没有 schema 就不该有 components.schemas：{:?}",
        doc["components"]
    );
}

/// 没有请求体的端点不得在 `components.schemas` 里留下孤儿定义。
///
/// 模式切成 `none` 之后残留的 schema 正是这种状态：界面上它还看得见（所以用户能
/// 删掉它），而导出时它没有 requestBody 可引用 —— 收集了却没人引用，
/// `components.schemas` 里就多出一份没人引用的定义。
#[test]
fn an_endpoint_without_a_request_body_leaves_no_orphan_component() {
    let ex = one_module(
        "m",
        vec![IrEndpoint {
            body: RequestBody {
                mode: BodyMode::None,
                content: String::new(),
                schema: Some("{\"type\":\"object\"}".into()),
            },
            ..ep("a", HttpMethod::Post, "/a")
        }],
    );
    let doc = doc_of(&ex);

    assert!(
        doc["components"].get("schemas").is_none(),
        "没有 requestBody 就没有 schema 可引，不该留下孤儿定义：{:?}",
        doc["components"]
    );
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
    let name = referenced_schema_name(&doc, "/a", "post");
    assert_eq!(doc["components"]["schemas"][&name]["limit"], "number");
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

/// OpenAPI 没有脚本的对应构造，故 IR 里有脚本就要**说出来**。
///
/// 「导出成功」四个字会让用户看不见自己少了什么。端点、集合、模块三处各出一条。
#[test]
fn scripts_are_reported_because_openapi_cannot_express_them() {
    let ex = Exchange {
        project_name: "demo".into(),
        modules: vec![IrModule {
            key: "m0".into(),
            name: "带脚本的模块".into(),
            scripts: IrScripts {
                pre_request_script: Some("模块前置".into()),
                test_script: None,
            },
            collections: vec![IrCollection {
                key: "m0c0".into(),
                name: "带脚本的集合".into(),
                scripts: IrScripts {
                    pre_request_script: None,
                    test_script: Some("集合测试".into()),
                },
                endpoints: vec![IrEndpoint {
                    scripts: IrScripts {
                        pre_request_script: Some("接口前置".into()),
                        test_script: None,
                    },
                    ..ep("接口", HttpMethod::Get, "/a")
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let out = to_document(&ex, DocFormat::Json).expect("导出应成功");

    let warns: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 3, "{:?}", out.diagnostics);
    let pointers: Vec<&str> = warns.iter().map(|d| d.pointer.as_str()).collect();
    assert!(pointers.contains(&"/paths/~1a/get"), "{pointers:?}");
    assert!(pointers.contains(&"/nodes/m0c0"), "{pointers:?}");
    assert!(pointers.contains(&"/nodes/m0"), "{pointers:?}");
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
