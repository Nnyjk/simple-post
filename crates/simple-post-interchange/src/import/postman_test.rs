//! Postman v2.1 → IR：树、URL 拆分、body 模式、模板变量与畸形输入。
//!
//! 断言的是**线形状**（key 指针、severity、dropped 文本），因为这三样是外部真的会读的东西：
//! 落库侧拿 key 当选择集、UI 拿 severity 分级、用户拿 dropped 回到源文件。

use serde_json::{json, Value};
use simple_post_core::domain::{ApiKeyLocation, AuthType, BodyMode};

use super::*;
use crate::diagnostics::Severity;
use crate::ir::IrEndpoint;

/// 把一棵交换里的端点展平，顺序 = 模块顺序 + 其内声明顺序。
///
/// 顶层散落的请求现在**共用一个模块**（见 `LOOSE_KEY`），所以按 `modules[i].endpoints[0]`
/// 读会反复读到同一个请求。用这个走查器才不会把「形状变了」误读成「用例坏了」。
fn endpoints_in_order(ex: &Exchange) -> Vec<&IrEndpoint> {
    let mut out = Vec::new();
    let mut stack: Vec<&IrCollection> = ex.modules.iter().flat_map(|m| &m.collections).collect();
    for m in &ex.modules {
        out.extend(m.endpoints.iter());
    }
    while let Some(c) = stack.pop() {
        out.extend(c.endpoints.iter());
        stack.extend(c.collections.iter().rev());
    }
    out
}

/// 一份真实形状的集合：一个顶层 folder（内含 sub-folder 与请求）、一个直接挂顶层的请求。
fn collection_fixture() -> Value {
    json!({
        "info": {
            "name": "订单服务",
            "description": "order api",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [
            {
                "name": "订单模块",
                "item": [
                    {
                        "name": "订单查询",
                        "item": [
                            {
                                "name": "按 ID",
                                "item": [
                                    {
                                        "name": "查单个订单",
                                        "request": {
                                            "method": "GET",
                                            "header": [
                                                { "key": "X-Trace", "value": "on" },
                                                { "key": "X-Off", "value": "1", "disabled": true }
                                            ],
                                            "url": {
                                                "raw": "https://api.example.com/orders/{{order_id}}?verbose=true",
                                                "protocol": "https",
                                                "host": ["api", "example", "com"],
                                                "path": ["orders", "{{order_id}}"],
                                                "query": [
                                                    { "key": "verbose", "value": "true" }
                                                ]
                                            }
                                        }
                                    }
                                ]
                            }
                        ]
                    },
                    {
                        "name": "建单",
                        "request": {
                            "method": "POST",
                            "auth": {
                                "type": "bearer",
                                "bearer": [{ "key": "token", "value": "{{api_token}}" }]
                            },
                            "body": {
                                "mode": "raw",
                                "raw": "{\"sku\":\"A1\"}",
                                "options": { "raw": { "language": "json" } }
                            },
                            "url": {
                                "raw": "https://api.example.com/orders",
                                "protocol": "https",
                                "host": ["api", "example", "com"],
                                "path": ["orders"]
                            }
                        }
                    }
                ]
            },
            {
                "name": "根级健康检查",
                "request": {
                    "method": "GET",
                    "url": "https://api.example.com/healthz"
                }
            }
        ],
        "variable": [
            { "key": "base", "value": "https://api.example.com" }
        ]
    })
}

fn parse_ok(v: &Value) -> ParsedExchange {
    parse(v).expect("这份 fixture 必须是合法的 Postman 文档")
}

#[test]
fn a_real_collection_lands_as_modules_collections_and_endpoints() {
    let parsed = parse_ok(&collection_fixture());
    let ex = &parsed.exchange;

    assert_eq!(ex.project_name, "订单服务");
    assert_eq!(ex.project_description.as_deref(), Some("order api"));
    assert!(
        ex.diagnostics.is_empty(),
        "干净文档不该有诊断：{:?}",
        ex.diagnostics
    );

    // 顶层每个 **folder** 各成一个模块。
    assert_eq!(ex.modules.len(), 2);
    let m = &ex.modules[0];
    assert_eq!(m.key, "/item/0");
    assert_eq!(m.name, "订单模块");
    assert_eq!(m.collections.len(), 1);
    assert_eq!(m.collections[0].key, "/item/0/item/0");
    assert_eq!(m.collections[0].name, "订单查询");

    // folder 逐层成为集合，请求落进它所在的那一层。
    let sub = &m.collections[0].collections[0];
    assert_eq!(sub.key, "/item/0/item/0/item/0");
    assert_eq!(sub.name, "按 ID");
    let ep = &sub.endpoints[0];
    assert_eq!(ep.key, "/item/0/item/0/item/0/item/0/request");
    assert_eq!(ep.name, "查单个订单");
    assert_eq!(ep.method.as_str(), "GET");
    assert_eq!(ep.path, "/orders/{{order_id}}");
    assert_eq!(ep.headers.len(), 2);
    assert_eq!(ep.headers[0].key, "X-Trace");
    assert!(ep.headers[0].enabled);
    // `disabled` 必须落到 enabled 上，否则用户禁用的头会被发出去。
    assert!(!ep.headers[1].enabled);
    assert_eq!(ep.params.len(), 1);
    assert_eq!(ep.params[0].key, "verbose");
    assert!(ep.params[0].enabled);

    // 同一模块下的第二个请求挂在模块上（endpoint 可以直接挂模块）。
    assert_eq!(m.endpoints.len(), 1);
    let create = &m.endpoints[0];
    assert_eq!(create.key, "/item/0/item/1/request");
    assert_eq!(create.name, "建单");
    assert_eq!(create.method.as_str(), "POST");
    assert_eq!(create.body.mode, BodyMode::Json);
    assert_eq!(create.body.content, "{\"sku\":\"A1\"}");
    assert_eq!(create.auth.kind, AuthType::Bearer);
    assert_eq!(create.auth.bearer.as_deref(), Some("{{api_token}}"));

    // 直接挂在顶层的请求并进**同一个**模块，名字取集合名 —— 不是各自一个集合。
    let root_module = &ex.modules[1];
    assert_eq!(root_module.key, "/item");
    assert_eq!(root_module.name, "订单服务");
    assert_eq!(root_module.endpoints.len(), 1);
    assert_eq!(root_module.endpoints[0].key, "/item/1/request");
    assert_eq!(root_module.endpoints[0].path, "/healthz");

    // 同一 host 在模块内只算一台服务。
    assert_eq!(m.servers.len(), 1);
    assert_eq!(m.servers[0].service, "api.example.com");
    assert_eq!(m.servers[0].url, "https://api.example.com");
    assert_eq!(ex.endpoint_count(), 3);
}

#[test]
fn url_splits_into_path_params_and_module_server() {
    let parsed = parse_ok(&collection_fixture());
    let ep = &parsed.exchange.modules[0].collections[0].collections[0].endpoints[0];
    // raw 里的 `?` 之后归 params，host 归模块的 servers，path 里两者都不该剩。
    assert_eq!(ep.path, "/orders/{{order_id}}");
    assert!(!ep.path.contains("?"));
    assert!(!ep.path.contains("api.example.com"));
    assert_eq!(ep.params.len(), 1);
    assert_eq!(ep.params[0].value, "true");
    assert_eq!(
        parsed.exchange.modules[0].servers[0].url,
        "https://api.example.com"
    );

    // 没有 raw 时由 protocol/host/path 拼回，同样要拆出 host。
    let no_raw = json!({
        "info": { "name": "n" },
        "item": [{
            "name": "x",
            "request": {
                "method": "GET",
                "url": {
                    "protocol": "http",
                    "host": ["localhost", "8080"],
                    "path": ["ping"],
                    "query": [{ "key": "a", "value": "1" }, { "key": "b", "value": "2", "disabled": true }]
                }
            }
        }]
    });
    let parsed = parse_ok(&no_raw);
    let ep = &parsed.exchange.modules[0].endpoints[0];
    assert_eq!(ep.path, "/ping");
    // 端口不是服务名的一部分。
    assert_eq!(parsed.exchange.modules[0].servers[0].service, "localhost");
    assert_eq!(
        parsed.exchange.modules[0].servers[0].url,
        "http://localhost:8080"
    );
    assert_eq!(ep.params.len(), 2);
    assert!(ep.params[0].enabled);
    assert!(!ep.params[1].enabled);

    // host 是整体占位：拆不出服务，整串留在 path 里，并出诊断。
    let placeholder = json!({
        "info": { "name": "n" },
        "item": [{
            "name": "x",
            "request": { "method": "GET", "url": "{{baseUrl}}/users" }
        }]
    });
    let parsed = parse_ok(&placeholder);
    let ex = &parsed.exchange;
    assert_eq!(ex.modules[0].endpoints[0].path, "{{baseUrl}}/users");
    assert!(ex.modules[0].servers.is_empty());
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(ex.diagnostics[0].pointer, "/item/0/request/url");
}

#[test]
fn text_only_formdata_becomes_a_multipart_part_array() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "上传",
            "request": {
                "method": "POST",
                "body": {
                    "mode": "formdata",
                    "formdata": [
                        { "key": "field", "value": "1", "type": "text" },
                        { "key": "note", "value": "hello" }
                    ]
                },
                "url": "https://api.example.com/up"
            }
        }]
    }));

    let body = &parsed.exchange.modules[0].endpoints[0].body;
    assert_eq!(body.mode, BodyMode::Multipart);
    let parts: Vec<Part> = serde_json::from_str(&body.content).expect("content 必须是分片数组");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "field");
    assert_eq!(parts[0].value, "1");
    assert_eq!(parts[1].name, "note");
    assert_eq!(parts[1].value, "hello");
    // 文本分片不带这两个位；写出的 null 会让 TS / Rust 的分片解析判成非法输入并整个丢掉。
    assert!(parts[0].filename.is_none());
    assert!(parts[0].content_type.is_none());
    assert!(!body.content.contains("null"));
}

#[test]
fn a_formdata_file_field_is_reported_and_dropped() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "上传",
            "request": {
                "method": "POST",
                "body": {
                    "mode": "formdata",
                    "formdata": [
                        { "key": "field", "value": "1", "type": "text" },
                        { "key": "avatar", "type": "file", "src": "C:/tmp/a.png" }
                    ]
                },
                "url": "https://api.example.com/up"
            }
        }]
    }));

    let ex = &parsed.exchange;
    assert_eq!(ex.diagnostics.len(), 1);
    let d = &ex.diagnostics[0];
    assert_eq!(d.severity, Severity::Warn);
    assert_eq!(d.pointer, "/item/0/request/body/formdata/1");
    assert_eq!(d.dropped.as_deref(), Some("multipart file 字段 avatar"));

    let body = &ex.modules[0].endpoints[0].body;
    let parts: Vec<Part> = serde_json::from_str(&body.content).expect("content 必须是分片数组");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "field");
}

#[test]
fn other_body_modes_land_where_the_sender_expects_them() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "urlencoded",
                "request": {
                    "method": "POST",
                    "body": {
                        "mode": "urlencoded",
                        "urlencoded": [
                            { "key": "a", "value": "1" },
                            { "key": "b", "value": "2" },
                            { "key": "off", "value": "3", "disabled": true }
                        ]
                    },
                    "url": "https://api.example.com/a"
                }
            },
            {
                "name": "raw text",
                "request": {
                    "method": "POST",
                    "body": { "mode": "raw", "raw": "hello" },
                    "url": "https://api.example.com/b"
                }
            },
            {
                "name": "graphql",
                "request": {
                    "method": "POST",
                    "body": { "mode": "graphql", "graphql": { "query": "{ a }" } },
                    "url": "https://api.example.com/c"
                }
            },
            {
                "name": "空 body",
                "request": { "method": "POST", "url": "https://api.example.com/d" }
            }
        ]
    }));

    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    assert_eq!(eps.len(), 4, "四条顶层散落的请求都进了同一个模块");
    assert_eq!(eps[0].body.mode, BodyMode::Form);
    assert_eq!(eps[0].body.content, "a=1&b=2");
    assert_eq!(eps[1].body.mode, BodyMode::Raw);
    assert_eq!(eps[1].body.content, "hello");
    assert_eq!(eps[2].body.mode, BodyMode::None);
    assert_eq!(eps[3].body.mode, BodyMode::None);

    // graphql 没有对应形态，必须出声。
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(ex.diagnostics[0].dropped.as_deref(), Some("graphql 请求体"));
}

#[test]
fn auth_payloads_are_only_read_for_the_selected_scheme() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "basic",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "basic",
                        "basic": [
                            { "key": "username", "value": "u" },
                            { "key": "password", "value": "p" }
                        ]
                    },
                    "url": "https://api.example.com/a"
                }
            },
            {
                "name": "apikey query",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "apikey",
                        "apikey": [
                            { "key": "key", "value": "X-Key" },
                            { "key": "value", "value": "k-1" },
                            { "key": "in", "value": "query" }
                        ]
                    },
                    "url": "https://api.example.com/b"
                }
            },
            {
                "name": "apikey cookie",
                "request": {
                    "method": "GET",
                    "auth": {
                        "type": "apikey",
                        "apikey": [{ "key": "in", "value": "cookie" }]
                    },
                    "url": "https://api.example.com/c"
                }
            },
            {
                "name": "noauth",
                "request": {
                    "method": "GET",
                    "auth": { "type": "noauth" },
                    "url": "https://api.example.com/d"
                }
            }
        ]
    }));

    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    assert_eq!(eps.len(), 4);
    let basic = &eps[0].auth;
    assert_eq!(basic.kind, AuthType::Basic);
    assert_eq!(basic.basic.as_ref().unwrap().username, "u");
    assert_eq!(basic.basic.as_ref().unwrap().password, "p");

    let apikey = &eps[1].auth;
    assert_eq!(apikey.kind, AuthType::Apikey);
    let ak = apikey.apikey.as_ref().unwrap();
    assert_eq!(ak.key, "X-Key");
    assert_eq!(ak.value, "k-1");
    assert_eq!(ak.in_location, ApiKeyLocation::Query);

    // cookie 位置表达不了：整段丢弃 + 出声，不猜成 header。
    assert_eq!(eps[2].auth.kind, AuthType::None);
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(
        ex.diagnostics[0].dropped.as_deref(),
        Some("apikey auth（in: cookie）")
    );

    assert_eq!(eps[3].auth.kind, AuthType::None);
}

#[test]
fn unresolvable_template_tokens_become_diagnostics() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "带怪变量的请求",
            "request": {
                "method": "GET",
                "header": [{ "key": "X-When", "value": "{{$timestamp}}" }],
                "url": "https://api.example.com/{{my-var}}"
            }
        }]
    }));

    let ex = &parsed.exchange;
    let by_dropped = |d: &str| -> &crate::diagnostics::Diagnostic {
        ex.diagnostics
            .iter()
            .find(|x| x.dropped.as_deref() == Some(d))
            .unwrap_or_else(|| panic!("没有关于 {d} 的诊断：{:?}", ex.diagnostics))
    };

    // 连字符：替换器的 `\w+` 不认，发送时会原样留在 URL 里。
    let bad = by_dropped("{{my-var}}");
    assert_eq!(bad.severity, Severity::Warn);
    assert_eq!(bad.pointer, "/item/0/request/url");
    assert!(bad.message.contains("不会被替换"));

    // Postman 动态变量：本仓库没有对应物，丢弃不影响功能。
    let dyn_var = by_dropped("{{$timestamp}}");
    assert_eq!(dyn_var.severity, Severity::Info);
    assert_eq!(dyn_var.pointer, "/item/0/request/header");

    assert_eq!(ex.diagnostics.len(), 2);
    // 合法的 `{{order_id}}` 形状不该有诊断。
    let ok = parse_ok(&collection_fixture());
    assert!(ok.exchange.diagnostics.is_empty());
}

#[test]
fn the_same_bad_token_is_reported_once_per_document() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "a",
                "request": { "method": "GET", "url": "https://api.example.com/{{my-var}}" }
            },
            {
                "name": "b",
                "request": { "method": "GET", "url": "https://api.example.com/{{my-var}}" }
            }
        ]
    }));
    assert_eq!(parsed.exchange.diagnostics.len(), 1);
}

#[test]
fn a_malformed_item_is_reported_and_the_rest_still_lands() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            "不是一个对象",
            { "name": "request 不是对象", "request": "GET" },
            { "name": "缺 url", "request": { "method": "GET" } },
            { "name": "既无 request 也无 item" },
            {
                "name": "好的那个",
                "request": { "method": "GET", "url": "https://api.example.com/ok" }
            }
        ]
    }));

    let ex = &parsed.exchange;
    // 四条坏条目各出一条 Error，其余照常。
    assert_eq!(ex.diagnostics.len(), 4);
    assert!(ex.diagnostics.iter().all(|d| d.severity == Severity::Error));
    assert_eq!(ex.modules.len(), 1);
    assert_eq!(ex.modules[0].endpoints.len(), 1);
    assert_eq!(ex.modules[0].endpoints[0].name, "好的那个");
    assert_eq!(ex.endpoint_count(), 1);
}

#[test]
fn a_non_object_document_is_rejected_rather_than_panicking() {
    assert!(matches!(
        parse(&json!([1, 2, 3])),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        parse(&json!("nope")),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn deep_folder_nesting_does_not_exhaust_the_stack() {
    // 12 层：递归也能扛，但工作栈是这里的不变量，故探到明显更深。
    const DEPTH: usize = 12;
    let mut node = json!({
        "name": "最底",
        "request": { "method": "GET", "url": "https://api.example.com/deep" }
    });
    for i in (0..DEPTH).rev() {
        node = json!({ "name": format!("层{i}"), "item": [node] });
    }
    let doc = json!({ "info": { "name": "深" }, "item": [node] });

    let parsed = parse_ok(&doc);
    assert_eq!(parsed.exchange.endpoint_count(), 1);

    // 一路走到底，key 逐层拼接：模块是 `/item/0`，每一层多一段 `/item/0`。
    // 请求的 key 比它所在的那一层多一段——那是这个条目自己的位置。
    let mut col = &parsed.exchange.modules[0].collections[0];
    for d in 2..DEPTH {
        assert_eq!(col.key, "/item/0".repeat(d));
        col = &col.collections[0];
    }
    let deepest = "/item/0".repeat(DEPTH);
    assert_eq!(col.key, deepest);
    assert_eq!(col.endpoints[0].key, format!("{deepest}/item/0/request"));
}

#[test]
fn collection_variables_become_an_environment_not_a_service() {
    let parsed = parse_ok(&collection_fixture());
    assert_eq!(parsed.environments.len(), 1);
    let env = &parsed.environments[0];
    assert!(!env.id.is_empty());
    // 落库方填 project_id，导入不猜。
    assert_eq!(env.project_id, "");
    assert_eq!(env.name, "订单服务");
    assert_eq!(
        env.variables.get("base").map(String::as_str),
        Some("https://api.example.com")
    );
    // 变量不是服务：导成服务会凭空造出一台不存在的机器。
    assert!(env.base_urls.is_empty());
    assert!(env.module_base_urls.is_empty());
    assert!(!env.is_active, "导入不抢激活位");

    // 没有 variable[] 就一个环境都不产出。
    let bare = parse_ok(&json!({ "info": { "name": "n" }, "item": [] }));
    assert!(bare.environments.is_empty());
}

#[test]
fn an_unnamed_item_falls_back_to_its_url_then_to_placeholder() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            { "request": { "method": "GET", "url": "https://api.example.com/nameless" } },
            { "request": { "method": "GET", "url": "https://api.example.com/x" } },
            {
                "item": [
                    { "name": "x", "request": { "method": "GET", "url": "https://api.example.com/y" } }
                ]
            },
            { "name": "空文件夹", "item": [] }
        ]
    }));
    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    // 两条顶层散落的请求并进同一个模块，模块名取集合名；**接口**自己的名字仍按 raw URL 兜底。
    assert_eq!(ex.modules.len(), 2, "两个散落请求并一个，再加那个文件夹");
    assert_eq!(ex.modules[0].key, LOOSE_KEY);
    assert_eq!(ex.modules[0].name, "n");
    assert_eq!(eps[0].name, "https://api.example.com/nameless");
    assert_eq!(eps[1].name, "https://api.example.com/x");
    // 文件夹没有请求可退，只能叫「未命名」。
    assert_eq!(ex.modules[1].name, "未命名");
    assert_eq!(eps[2].name, "x");
    // 空容器不进树：预览里挂一个什么都没有的模块只会让人以为少导了东西。
    assert_eq!(ex.endpoint_count(), 3);
}

/// 顶层散落的请求共用一个分组；folder 仍各成一个。
///
/// 这条与 `import::openapi::tests::each_tag_becomes_one_group_and_the_untagged_ones_share_another`
/// 是同一条纪律的两侧。曾经的形状是「每个顶层 item 一个模块」，于是一份把请求平铺在根上的
/// 集合（Postman 里很常见）导入后变成 N 个各含 1 个接口的集合。
#[test]
fn loose_top_level_requests_share_one_group_while_folders_stay_apart() {
    let parsed = parse_ok(&json!({
        "info": { "name": "订单服务" },
        "item": [
            { "name": "散落一", "request": { "method": "GET", "url": "/a" } },
            {
                "name": "文件夹甲",
                "item": [{ "name": "甲里的", "request": { "method": "GET", "url": "/b" } }]
            },
            { "name": "散落二", "request": { "method": "POST", "url": "/c" } },
            { "name": "文件夹乙", "item": [] }
        ]
    }));
    let ex = &parsed.exchange;

    assert_eq!(ex.modules.len(), 2, "散落的一组 + 文件夹甲；空文件夹不进树");
    assert_eq!(ex.modules[0].key, LOOSE_KEY);
    assert_eq!(ex.modules[0].name, "订单服务");
    assert_eq!(
        ex.modules[0].endpoints.len(),
        2,
        "两条散落请求在同一个分组里"
    );
    assert_eq!(ex.modules[0].endpoints[0].path, "/a");
    assert_eq!(ex.modules[0].endpoints[1].path, "/c");
    // 分组位置取**第一条**散落请求出现的地方，后面的 folder 排在它之后。
    assert_eq!(ex.modules[1].key, "/item/1");
    assert_eq!(ex.modules[1].name, "文件夹甲");
    assert!(
        ex.modules[1].collections.is_empty(),
        "这个 folder 里全是请求，没有更深的层级"
    );
    assert_eq!(ex.modules[1].endpoints.len(), 1);
    assert_eq!(ex.endpoint_count(), 3);
}

/// 测试侧的 `MultipartPart` 镜像：与 `src/lib/multipart.ts` 的线格式一致，
/// 故这里能反序列化成功，正说明导入写出的 JSON 发送侧读得懂。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}
