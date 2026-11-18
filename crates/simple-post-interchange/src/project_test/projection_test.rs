//! `domain → ir → domain` 往返、投影的幂等与「不回退」的服务取值。
//!
//! 导出入口是 `module_to_ir`（一个模块的整棵子树），故这里的 IR 都从模块这一层取。

use simple_post_core::db::{self, DbPool};

use super::module_to_ir;
use super::support::{
    add_endpoint, add_endpoint_without_detail, add_module, seed_project, temp_pool,
};
use crate::ir::{Exchange, IrEndpoint};

/// 取一个模块的 IR。
fn project(
    pool: &DbPool,
    project_id: &str,
    module_id: &str,
    environment_id: Option<&str>,
) -> Exchange {
    module_to_ir(pool, project_id, module_id, environment_id).expect("project")
}

#[test]
fn domain_to_ir_to_domain_keeps_the_request() {
    let pool = temp_pool("rt");
    let s = seed_project(&pool);
    let node_id = add_endpoint(&pool, &s, &s.module_id);

    let ex = project(&pool, &s.project_id, &s.module_id, Some(&s.env_id));
    assert_eq!(ex.environment_id.as_deref(), Some(s.env_id.as_str()));

    let ir_ep: &IrEndpoint = ex.modules[0]
        .endpoints
        .iter()
        .find(|e| e.path == "/users/{{id}}")
        .expect("endpoint projected");
    // host 不在 endpoint 上：IR 存的是 path，地址另挂在模块那一格与端点自己身上。
    assert!(!ir_ep.path.contains("example.com"));

    // ir → 落库 → 读回，与原始行逐字段比。目标项目在同一个库里。
    let target = seed_project(&pool);
    crate::commit::commit(
        &pool,
        &target.project_id,
        crate::commit::CommitTarget::ProjectRoot,
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect("recommit");

    let before = db::with_conn(&pool, |c| db::endpoint_details::get(c, &node_id)).expect("orig");
    let after = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &target.project_id)
    })
    .expect("read back");
    assert_eq!(after.len(), 1);
    let after = &after[0];

    assert_eq!(after.method, before.method);
    assert_eq!(after.url, before.url);
    assert_eq!(after.params, before.params);
    assert_eq!(after.headers, before.headers);
    assert_eq!(after.body, before.body);
    assert_eq!(after.auth, before.auth);
    assert_eq!(after.docs, before.docs);
    assert_eq!(after.notes, before.notes);
    assert_eq!(after.tags, before.tags);
}

#[test]
fn projecting_twice_gives_the_same_answer() {
    let pool = temp_pool("idem");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);

    let first = project(&pool, &s.project_id, &s.module_id, Some(&s.env_id));
    let second = project(&pool, &s.project_id, &s.module_id, Some(&s.env_id));

    assert_eq!(first.modules.len(), second.modules.len());
    assert_eq!(
        first.modules[0].endpoints[0].path,
        second.modules[0].endpoints[0].path
    );
    assert_eq!(first.modules[0].endpoints[0].path, "/users/{{id}}");
}

#[test]
fn the_address_is_read_per_module_with_no_fallback() {
    let pool = temp_pool("srv");
    let s = seed_project(&pool);
    let without_address = add_module(&pool, &s.project_id, "没有地址的模块");

    db::with_tx(&pool, |conn| {
        db::environments::set_module_base_url(
            conn,
            &s.env_id,
            &s.module_id,
            Some("https://mod-a.example.com"),
            None,
        )
    })
    .expect("set url");

    // 配了地址的那个模块读得到自己那一格。
    let with_address = project(&pool, &s.project_id, &s.module_id, Some(&s.env_id));
    assert_eq!(
        with_address.modules[0]
            .base_url
            .as_ref()
            .map(|c| c.url.clone())
            .as_deref(),
        Some("https://mod-a.example.com")
    );

    // 没配的那个模块没有地址，不借用别的模块的。
    let no_address = project(&pool, &s.project_id, &without_address, Some(&s.env_id));
    assert!(
        no_address.modules[0].base_url.is_none(),
        "未配置的模块不得借用别的模块的地址"
    );
}

#[test]
fn an_environment_from_another_project_is_not_used() {
    let pool = temp_pool("cross");
    let a = seed_project(&pool);
    let b = seed_project(&pool);
    assert_ne!(a.project_id, b.project_id);

    // 指名别的项目的环境：地址会跨项目串过来，故当作「没选环境」。
    let ex = project(&pool, &b.project_id, &b.module_id, Some(&a.env_id));
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.base_url.is_none()));
}

#[test]
fn a_project_with_no_default_environment_projects_no_address() {
    let pool = temp_pool("noenv");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);

    // 删掉唯一的环境，再不指名任何环境：投影不猜，地址为空。
    db::with_tx(&pool, |conn| db::environments::delete(conn, &s.env_id)).expect("delete");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.base_url.is_none()));
    // 树本身照常投影出来：没有地址不影响接口清单。
    assert_eq!(ex.endpoint_count(), 1);
}

/// 两表不一致时导出要**出声**，不是把那个接口悄悄丢掉。
///
/// 节点在、detail 行不在，这个接口就打不出请求。导出侧要把这件事报成一条 Error：
/// 静默丢掉只会让用户看到「导出的文档里少了几个接口」，没有任何线索指向少了谁。
///
/// 诊断的指针是 `/nodes/<节点 id>`，用户据此能定位到是哪一个接口出的问题。
#[test]
fn an_endpoint_without_a_detail_row_is_reported_not_dropped_silently() {
    let pool = temp_pool("nodetail");
    let s = seed_project(&pool);
    let orphan = add_endpoint_without_detail(&pool, &s, &s.module_id);

    let ex = project(&pool, &s.project_id, &s.module_id, Some(&s.env_id));

    assert!(
        ex.modules
            .iter()
            .all(|m| m.endpoints.iter().all(|e| e.key != orphan)),
        "缺 detail 行的接口不该出现在导出结果里"
    );
    let errors: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", ex.diagnostics);
    assert_eq!(errors[0].pointer, format!("/nodes/{orphan}"));
}

/// 会话式传输没有「一次请求一份响应」可描述：导出跳过并按诊断说明。
#[test]
fn streaming_endpoints_are_skipped_with_a_diagnostic() {
    use simple_post_core::domain::{EndpointDetailPatch, RequestProtocol};
    use simple_post_core::error::AppError;

    let pool = temp_pool("skip-protocol");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::Sse)),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as sse");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    assert_eq!(ex.endpoint_count(), 0, "SSE 端点不能出现在导出里");
    assert!(
        ex.diagnostics.iter().any(|d| d.message.contains("已跳过")
            && d.message.contains("sse")
            && d.severity == crate::diagnostics::Severity::Warn),
        "跳过必须留下 warn 级诊断：{:?}",
        ex.diagnostics
    );
}

/// GraphQL 端点导出成真正会发出的请求：POST、JSON 正文、`Content-Type`。
#[test]
fn graphql_endpoints_export_as_the_post_they_send() {
    use simple_post_core::domain::{
        BodyMode, EndpointDetailPatch, GraphQlRequest, HttpMethod, RequestProtocol,
    };
    use simple_post_core::error::AppError;

    let pool = temp_pool("graphql");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::GraphQl)),
                graphql: Some(Some(GraphQlRequest {
                    query: "{ me { id } }".into(),
                    variables: "{\"b\":1,\"a\":2}".into(),
                    operation_name: Some("Me".into()),
                })),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as graphql");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    let endpoint = &ex.modules[0].endpoints[0];
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert_eq!(endpoint.body.mode, BodyMode::Json);
    let body: serde_json::Value = serde_json::from_str(&endpoint.body.content).expect("json body");
    assert_eq!(body["query"], "{ me { id } }");
    assert_eq!(body["variables"], serde_json::json!({"b": 1, "a": 2}));
    assert_eq!(body["operationName"], "Me");
    assert!(
        endpoint.headers.iter().any(|h| {
            h.key.eq_ignore_ascii_case("content-type") && h.value == "application/json"
        }),
        "导出必须带上 Content-Type：{:?}",
        endpoint.headers
    );
    assert_eq!(
        endpoint.body_example.as_deref(),
        Some(endpoint.body.content.as_str()),
        "投影出来的正文必须一路带到导出层，否则文档里没有真正的请求"
    );
    assert_eq!(
        endpoint.body.schema, None,
        "GraphQL 不该沿用切换协议前那份 HTTP 载荷 schema"
    );
    assert!(
        ex.diagnostics
            .iter()
            .any(|d| d.pointer == format!("/nodes/{id}/body/schema")
                && d.severity == crate::diagnostics::Severity::Info),
        "丢掉旧 schema 必须留下一条说明：{:?}",
        ex.diagnostics
    );

    // 文档层：拼好的正文必须真的出现在导出的 OpenAPI 里，而不是只活在 IR 里。
    let out =
        crate::openapi::to_document(&ex, crate::openapi::DocFormat::Json).expect("openapi export");
    let doc: serde_json::Value = serde_json::from_str(&out.content).expect("valid json");
    // 导出层把 `{{name}}` 归一成 `{name}`，测试要按同一个键去取。
    let path_key = format!("/{}", endpoint.path.trim_start_matches('/'))
        .replace("{{", "{")
        .replace("}}", "}");
    let example =
        &doc["paths"][path_key]["post"]["requestBody"]["content"]["application/json"]["example"];
    assert_eq!(example["query"], "{ me { id } }", "导出文档：{doc}");
    assert_eq!(example["variables"], serde_json::json!({"b": 1, "a": 2}));
    assert_eq!(example["operationName"], "Me");
}

/// WebSocket 与 SSE 同一档：导出跳过并说明。
#[test]
fn websocket_endpoints_are_skipped_with_a_diagnostic() {
    use simple_post_core::domain::{EndpointDetailPatch, RequestProtocol};
    use simple_post_core::error::AppError;

    let pool = temp_pool("skip-ws");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::WebSocket)),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as websocket");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    assert_eq!(ex.endpoint_count(), 0, "WebSocket 端点不能出现在导出里");
    assert!(ex.diagnostics.iter().any(|d| {
        d.message.contains("已跳过")
            && d.message.contains("websocket")
            && d.severity == crate::diagnostics::Severity::Warn
    }));
}

/// 空 query 仍然导出一个 POST，但必须留 warn，且不能把 `{"query":""}` 当成可发送的实例。
#[test]
fn an_empty_graphql_query_is_exported_with_a_warning_and_no_example() {
    use simple_post_core::domain::{EndpointDetailPatch, RequestProtocol};
    use simple_post_core::error::AppError;

    let pool = temp_pool("graphql-empty");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::GraphQl)),
                graphql: None,
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as graphql without a config");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    let endpoint = &ex.modules[0].endpoints[0];
    assert_eq!(endpoint.method, simple_post_core::domain::HttpMethod::Post);
    let body: serde_json::Value = serde_json::from_str(&endpoint.body.content).expect("json body");
    assert_eq!(body, serde_json::json!({ "query": "" }));
    assert_eq!(endpoint.body_example, None, "没有 query 就没有可发送的示例");
    assert!(
        ex.diagnostics.iter().any(|d| {
            d.pointer == format!("/nodes/{id}/graphql/query")
                && d.severity == crate::diagnostics::Severity::Warn
        }),
        "空 query 必须留下 warn：{:?}",
        ex.diagnostics
    );
}

/// query 只有空白同样是空的：诊断、不填 example。
#[test]
fn a_whitespace_graphql_query_is_treated_as_empty() {
    use simple_post_core::domain::{EndpointDetailPatch, GraphQlRequest, RequestProtocol};
    use simple_post_core::error::AppError;

    let pool = temp_pool("graphql-blank");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::GraphQl)),
                graphql: Some(Some(GraphQlRequest {
                    query: "  \n ".into(),
                    variables: String::new(),
                    operation_name: None,
                })),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as graphql with blank query");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    let endpoint = &ex.modules[0].endpoints[0];
    assert_eq!(endpoint.body_example, None);
    assert!(
        ex.diagnostics
            .iter()
            .any(|d| d.pointer == format!("/nodes/{id}/graphql/query")
                && d.severity == crate::diagnostics::Severity::Warn),
        "{:?}",
        ex.diagnostics
    );
}

/// 停用的 `Content-Type` 不算写过：导出补一条启用的，停用那条原样保留。
#[test]
fn a_disabled_content_type_still_gets_a_json_one_for_graphql() {
    use simple_post_core::domain::{
        EndpointDetailPatch, GraphQlRequest, KeyValue, RequestProtocol,
    };
    use simple_post_core::error::AppError;

    let pool = temp_pool("graphql-disabled-ct");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::GraphQl)),
                graphql: Some(Some(GraphQlRequest {
                    query: "{ ping }".into(),
                    variables: String::new(),
                    operation_name: None,
                })),
                headers: Some(vec![KeyValue {
                    id: "h-disabled".into(),
                    key: "Content-Type".into(),
                    value: "text/plain".into(),
                    enabled: false,
                    description: None,
                }]),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as graphql");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    let headers = &ex.modules[0].endpoints[0].headers;
    assert!(
        headers.iter().any(|h| {
            h.enabled && h.key.eq_ignore_ascii_case("content-type") && h.value == "application/json"
        }),
        "缺一条启用的 application/json：{headers:?}"
    );
    assert!(
        headers.iter().any(|h| {
            !h.enabled && h.key.eq_ignore_ascii_case("content-type") && h.value == "text/plain"
        }),
        "停用的那条要原样保留：{headers:?}"
    );
}

/// 用户启用的 `Content-Type` 不被覆盖：只在缺的时候补。
#[test]
fn an_enabled_content_type_keeps_the_users_value_for_graphql() {
    use simple_post_core::domain::{
        EndpointDetailPatch, GraphQlRequest, KeyValue, RequestProtocol,
    };
    use simple_post_core::error::AppError;

    let pool = temp_pool("graphql-enabled-ct");
    let s = seed_project(&pool);
    let id = add_endpoint(&pool, &s, &s.module_id);
    db::with_tx(&pool, |conn| {
        db::endpoint_details::update(
            conn,
            &id,
            EndpointDetailPatch {
                protocol: Some(Some(RequestProtocol::GraphQl)),
                graphql: Some(Some(GraphQlRequest {
                    query: "{ ping }".into(),
                    variables: String::new(),
                    operation_name: None,
                })),
                headers: Some(vec![KeyValue {
                    id: "h-ct".into(),
                    key: "content-type".into(),
                    value: "application/graphql+json".into(),
                    enabled: true,
                    description: None,
                }]),
                ..Default::default()
            },
        )?;
        Ok::<(), AppError>(())
    })
    .expect("mark the endpoint as graphql");

    let ex = project(&pool, &s.project_id, &s.module_id, None);
    let headers = &ex.modules[0].endpoints[0].headers;
    let content_types: Vec<&KeyValue> = headers
        .iter()
        .filter(|h| h.key.eq_ignore_ascii_case("content-type"))
        .collect();
    assert_eq!(content_types.len(), 1, "不该再补一条：{headers:?}");
    assert_eq!(content_types[0].value, "application/graphql+json");
    assert!(content_types[0].enabled);
}
