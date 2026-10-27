//! 用 **OpenAPI 3.0 的官方数据模型**校验自己的导出产物。
//!
//! 为什么自家解析器不算数：本 crate 的导入器也是我们自己写的，它对自家产物永远宽容。
//! 导出器把 `responses` 漏掉，导入器不会报错——它只是安静地少一个响应，而那份文档
//! 已经不合法了。往返测试能证明「我们能读回自己写的东西」，**证明不了「那是合法文档」**。
//!
//! [`openapiv3::OpenAPI`] 是第三方模型，它的 `Deserialize` 照 OpenAPI 3.0 的 REQUIRED
//! 列表来：缺 `info.title`、缺 `info.version`、operation 缺 `responses`，都在这里炸。
//!
//! **读的是 `fixtures/sample-exchange.json`，不是这里手搭的树。** `examples/export_sample.rs`
//! 读同一份文件交给 Redocly，故 linter 看到的与这里校验过的是同一棵树——两份构造各写一遍
//! 的话，它们迟早只覆盖一半。
//!
//! 夹具覆盖面：八个合法动词 + 一个集合外的动词、四种 body mode、三种可映射的鉴权、
//! 两层嵌套集合、路径参数、配了服务与没配服务的两个模块。

use std::collections::BTreeMap;

use crate::import::Format;
use crate::ir::Exchange;
use crate::openapi::{to_document, DocFormat};

const FIXTURE: &str = include_str!("../fixtures/sample-exchange.json");

fn sample() -> Exchange {
    serde_json::from_str(FIXTURE).expect("夹具已与 IR 类型脱节——先跑 `export_sample -- --ir`")
}

/// 夹具里的接口数，以及其中被导出的那部分。少改一处就会在这里红。
const FIXTURE_ENDPOINTS: usize = 20;
/// `PROPFIND` 在固定动词键集之外，被带诊断跳过，故产物里比夹具少一个。
const SKIPPED_VERBS: usize = 1;

/// 两种编码都要过。YAML 走 `serde_norway` 直接反序列化进同一个模型 —— 它用的是
/// 真正的 serde trait，所以第三方类型不需要为它做适配。
fn parse_model(text: &str, format: DocFormat) -> openapiv3::OpenAPI {
    match format {
        DocFormat::Json => serde_json::from_str::<openapiv3::OpenAPI>(text)
            .unwrap_or_else(|e| panic!("JSON 产物不合 OpenAPI 3.0 模型：{e}\n---\n{text}")),
        DocFormat::Yaml => serde_norway::from_str::<openapiv3::OpenAPI>(text)
            .unwrap_or_else(|e| panic!("YAML 产物不合 OpenAPI 3.0 模型：{e}\n---\n{text}")),
    }
}

#[test]
fn both_encodings_parse_into_the_official_openapi_model() {
    let ex = sample();
    let json = to_document(&ex, DocFormat::Json).expect("json");
    let yaml = to_document(&ex, DocFormat::Yaml).expect("yaml");

    let from_json = parse_model(&json.content, DocFormat::Json);
    let from_yaml = parse_model(&yaml.content, DocFormat::Yaml);

    assert_eq!(
        from_json, from_yaml,
        "JSON 与 YAML 是同一份文档的两种编码，模型层也必须一样"
    );
    assert!(
        from_json.openapi.starts_with("3.0"),
        "openapi 字段应声明 3.0.x，got: {}",
        from_json.openapi
    );
}

#[test]
fn the_required_fields_the_model_demands_are_present() {
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    // `openapiv3` 的 `Deserialize` 已经强制了这些存在；显式断言是为了让失败信息
    // 说的是「我们没写」而不是「第 N 个字段缺失」。
    assert!(!api.info.title.is_empty(), "info.title 是 REQUIRED");
    assert!(!api.info.version.is_empty(), "info.version 是 REQUIRED");

    let operations: Vec<_> = api.operations().collect();
    assert_eq!(
        operations.len(),
        FIXTURE_ENDPOINTS - SKIPPED_VERBS,
        "夹具里 {FIXTURE_ENDPOINTS} 个接口，减去 {SKIPPED_VERBS} 个集合外动词"
    );
    for (path, method, op) in &operations {
        assert!(
            !op.responses.responses.is_empty(),
            "responses 是每个 operation 的 REQUIRED 字段：{method} {path}"
        );
    }
}

#[test]
fn every_operation_has_exactly_the_one_synthetic_200() {
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    for (path, method, op) in api.operations() {
        let codes: Vec<String> = op
            .responses
            .responses
            .keys()
            .map(|c| c.to_string())
            .collect();
        assert_eq!(
            codes,
            vec!["200".to_string()],
            "{method} {path} 只应有合成的 200——编出来的响应体比没有更糟"
        );
        let response = op
            .responses
            .responses
            .values()
            .next()
            .expect("刚断言过非空");
        assert!(
            !response
                .as_item()
                .expect("合成响应不该是 $ref")
                .description
                .is_empty(),
            "response 必须有 description（OpenAPI 的 REQUIRED）"
        );
    }
}

#[test]
fn path_templates_use_braces_not_axum_colons() {
    // `{id}` 是 OpenAPI 的写法，`:id` 是 axum 的。混进后者会让 linter 与多数工具
    // 读不出路径参数，尽管模型本身不报错。
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    assert!(!doc.content.contains(":id"), "产物里出现了 axum 的 :id");

    let api = parse_model(&doc.content, DocFormat::Json);
    assert!(
        api.paths.paths.contains_key("/users/{id}"),
        "带路径参数的那条应当原样出现在 paths 里"
    );
}

#[test]
fn servers_land_on_the_operations_of_the_module_that_has_one() {
    // `servers` 写在 **Operation 层**而非文档根：OpenAPI 3.0 允许 operation 覆盖顶层
    // servers，而这里必须逐 operation 带——URL 是 per (环境 × 模块 × 服务) 的，
    // 提到根就等于让没有配 URL 的模块借到兄弟模块的地址。
    //
    // 顺带这条断言：**根级 `api.servers` 恒为空是正确的**，不是漏写。
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    assert!(
        api.servers.is_empty(),
        "文档根不该有 servers：URL 是逐模块的，提到根就变成全局默认"
    );

    let (with, without): (Vec<_>, Vec<_>) = api
        .operations()
        .partition(|(_, _, op)| !op.servers.is_empty());

    assert_eq!(
        without.len(),
        1,
        "只有「无服务模块」的 /health 不该有 servers，got: {:?}",
        without.iter().map(|(p, _, _)| *p).collect::<Vec<_>>()
    );
    assert_eq!(without[0].0, "/health");

    for (path, method, op) in &with {
        let urls: Vec<String> = op.servers.iter().map(|s| s.url.clone()).collect();
        assert_eq!(
            urls,
            vec![
                "https://api.acme.test/v1".to_string(),
                "https://upload.acme.test".to_string(),
            ],
            "{method} {path} 的 server 只能是自己模块那两条"
        );
    }
}

#[test]
fn a_verb_outside_the_fixed_set_is_skipped_and_diagnosed() {
    // 夹具里有一个 `PROPFIND`。它若被写进 `paths`，那份文档就不合法了——
    // 所以「跳过 + 说出来」两件事都要。
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    assert!(
        !api.paths.paths.contains_key("/files"),
        "集合外的动词不该出现在产物里"
    );
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.dropped.as_deref().is_some_and(|s| s.contains("PROPFIND"))),
        "跳过的动词必须带 dropping 说明，got: {:?}",
        doc.diagnostics
    );
}

#[test]
fn the_model_agrees_with_our_own_importer() {
    // 最后一根钉：两个视角都得认这份文档。不一致就说明生成器与解析器对 OpenAPI
    // 的理解已经分叉，而两边各自的单测都还是绿的。
    let ex = sample();
    for format in [DocFormat::Json, DocFormat::Yaml] {
        let doc = to_document(&ex, format).expect("generate");
        parse_model(&doc.content, format);
        crate::import::parse_text(Format::OpenApi3, &doc.content)
            .unwrap_or_else(|e| panic!("自家导入器读不回 {format:?} 产物：{e}"));
    }
}

/// 文档里的 `operationId` 必须互不相同。
///
/// 规范要求 `operationId` 在文档内唯一，而「动词 + 路径」的映射不是单射：
/// `/api/user-info` 与 `/api/user/info` 都可能映成 `get__api_user_info`。撞名之后多数
/// 代码生成器会把 `operationId` 直接变成方法名，于是生成出两个同名方法。
///
/// `openapiv3` 校验的是**模型**（不校验唯一性），Redocly 也没开那条规则，故这条
/// 测试就是那道门。夹具里放了一对这样的路径（`m0e9` 与 `m0e10`）。
#[test]
fn every_operation_id_is_unique_in_the_document() {
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (path, method, op) in api.operations() {
        let id = op.operation_id.clone().unwrap_or_default();
        assert!(!id.is_empty(), "{method} {path} 没有 operationId");
        if let Some(previous) = seen.insert(id.clone(), format!("{method} {path}")) {
            panic!("operationId 撞名了：{id} 同时属于「{previous}」与「{method} {path}」");
        }
    }
}

#[test]
fn the_fixture_still_covers_every_path_method() {
    // 导出器加了动词而夹具没跟上的话，Redocly 那一侧看不见它。成本是一条断言。
    let ex = sample();
    let doc = to_document(&ex, DocFormat::Json).expect("json");
    let api = parse_model(&doc.content, DocFormat::Json);

    let mut verbs: Vec<String> = api.operations().map(|(_, m, _)| m.to_string()).collect();
    verbs.sort();
    verbs.dedup();
    for expected in [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ] {
        assert!(
            verbs.iter().any(|v| v == expected),
            "夹具没覆盖 `{expected}`，linter 那一侧会看不见它。got: {verbs:?}"
        );
    }
}
