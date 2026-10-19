//! **往返**：导出器产出的文档，OpenAPI 导入器读回来。
//!
//! 这条链上任何一环只对着自己的期望值自洽都不够——导出器按 OpenAPI 写、导入器按
//! OpenAPI 读，两边各自测试全绿却互相不认得时，**没有任何一个测试会红**，而用户会看到
//! 「导出的文件导不回来」。故这里断言的是**同一份产物**在两侧的形状。
//!
//! 这也是唯一能覆盖 `paths` 键的：OpenAPI 的 `paths` 是固定动词键集，写错键名不会被
//! 任何 schema 校验拦住，只会在下一次导入时静默少掉一整个接口。

use simple_post_core::domain::HttpMethod;

use crate::import::{self, Format};
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule};
use crate::openapi::{to_document, DocFormat};
use crate::Severity;

/// 一棵有代表性的树：一个模块、两个直接接口、一个嵌套集合、集合里一个接口。
///
/// 方法覆盖 `paths` 的两种键位——直接挂在模块上的 `get`，与集合里带路径参数的 `delete`。
/// 嵌套集合是为了让「分组」这条边也参与往返：它落在 OpenAPI 的 tag 上，读回来时
/// 变成模块下的集合而不是模块下的接口。
fn round_trip_tree() -> Exchange {
    Exchange {
        project_name: "示例".into(),
        modules: vec![IrModule {
            key: "/item/0".into(),
            name: "用户".into(),
            endpoints: vec![
                endpoint("/item/0/item/0/request", HttpMethod::Get, "/users"),
                endpoint("/item/0/item/1/request", HttpMethod::Get, "/users/{id}"),
            ],
            collections: vec![IrCollection {
                key: "/item/0/item/2".into(),
                name: "详情".into(),
                endpoints: vec![endpoint(
                    "/item/0/item/2/item/0/request",
                    HttpMethod::Delete,
                    "/users/{id}",
                )],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn endpoint(key: &str, method: HttpMethod, path: &str) -> IrEndpoint {
    IrEndpoint {
        key: key.into(),
        method: method.clone(),
        path: path.into(),
        ..IrEndpoint::new(key, method, path)
    }
}

#[test]
fn json_and_yaml_are_the_same_document_in_two_encodings() {
    let ex = round_trip_tree();
    let as_json = to_document(&ex, DocFormat::Json).expect("json");
    let as_yaml = to_document(&ex, DocFormat::Yaml).expect("yaml");

    // 编码不同，**内容**必须相同。用导入器做这个比较最直接：它两个都读得回来，
    // 且读回来的形状一致，才说明 YAML 分支没有丢字段。
    let from_json = import::parse_text(Format::OpenApi3, &as_json.content).expect("parse json");
    let from_yaml = import::parse_text(Format::OpenApi3, &as_yaml.content).expect("parse yaml");

    assert_eq!(
        endpoint_signatures(&from_json.exchange),
        endpoint_signatures(&from_yaml.exchange),
        "同一棵树的 JSON 与 YAML 必须读回同一个结果"
    );
    assert_eq!(
        endpoint_signatures(&from_json.exchange),
        endpoint_signatures(&ex),
        "往返之后接口集合不该变"
    );
}

#[test]
fn the_generated_document_sniffs_as_openapi() {
    // 生成的文档必须能被 `import::sniff` 认出来，否则用户在弹窗里选「文件」
    // 之后得到的是「认不出这是什么格式」——而他选的正是我们刚导出的那个文件。
    let ex = round_trip_tree();
    for (format, text) in [
        (
            DocFormat::Json,
            to_document(&ex, DocFormat::Json).expect("json").content,
        ),
        (
            DocFormat::Yaml,
            to_document(&ex, DocFormat::Yaml).expect("yaml").content,
        ),
    ] {
        assert_eq!(
            import::sniff(&text).expect("sniff"),
            Format::OpenApi3,
            "{format:?} 产物没被认成 OpenAPI"
        );
    }
}

#[test]
fn a_round_trip_produces_no_errors() {
    // Warn 是允许的（一个接口可以降级），Error 不是：它意味着某个构造在写出时被丢掉，
    // 而用户看不见自己丢东西。
    let ex = round_trip_tree();
    for format in [DocFormat::Json, DocFormat::Yaml] {
        let doc = to_document(&ex, format).expect("generate");
        let errors: Vec<_> = doc
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{format:?} 产出里有 Error：{errors:?}");

        let reparsed = import::parse_text(Format::OpenApi3, &doc.content).expect("parse");
        let errors: Vec<_> = reparsed
            .exchange
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{format:?} 读回时有 Error：{errors:?}");
    }
}

#[test]
fn a_custom_verb_is_reported_rather_than_silently_dropped() {
    // `PROPFIND` 落在 OpenAPI 的固定动词键集之外。塞进去会让整份文档失效，
    // 所以必须跳过并**说出来**——静默跳过的后果是用户以为导出成功了。
    let mut ex = round_trip_tree();
    ex.modules[0].endpoints[0].method = HttpMethod::Custom("PROPFIND".into());
    ex.modules[0].endpoints[0].path = "/files".into();

    let doc = to_document(&ex, DocFormat::Json).expect("generate");
    // 动词写在 `dropped` 里，不在 `message` 里：后者是给人看的一句中文，动词与路径
    // 才是「到底丢了什么」的可核对记录。
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.dropped.as_deref().is_some_and(|s| s.contains("PROPFIND"))),
        "跳过的动词必须带 dropping 说明，got: {:?}",
        doc.diagnostics
    );

    // 剩下的那个接口还在。
    let reparsed = import::parse_text(Format::OpenApi3, &doc.content).expect("parse");
    let sigs = endpoint_signatures(&reparsed.exchange);
    assert_eq!(sigs.len(), 2, "只剩两个合法动词的接口，got: {sigs:?}");
    assert!(
        !sigs.iter().any(|(m, _)| m == "PROPFIND"),
        "被跳过的动词不该出现在产物里: {sigs:?}"
    );
}

#[test]
fn trace_survives_the_round_trip_as_a_custom_verb() {
    // 反向是通的：OpenAPI 的 `trace` 落成 `HttpMethod::Custom("TRACE")`，
    // 再导出时又认得出它是合法的 `trace` 键。少了这一段，用户导一次再导一次
    // 就会把自己的接口弄丢。
    let mut ex = round_trip_tree();
    ex.modules[0].endpoints[0].method = HttpMethod::Custom("TRACE".into());

    let doc = to_document(&ex, DocFormat::Json).expect("generate");
    assert!(
        !doc.diagnostics
            .iter()
            .any(|d| d.dropped.as_deref().is_some_and(|s| s.contains("TRACE"))),
        "trace 是合法动词，不该被当作集合外动词跳过：{:?}",
        doc.diagnostics
    );

    let reparsed = import::parse_text(Format::OpenApi3, &doc.content).expect("parse");
    assert!(
        endpoint_signatures(&reparsed.exchange)
            .iter()
            .any(|(m, _)| m == "TRACE"),
        "trace 读回来应当还是 TRACE; sigs={:?}",
        endpoint_signatures(&reparsed.exchange)
    );
}

/// 把一棵 IR 压成排序后的 `(method, path)` 列表。顺序在两侧都可能变（导出按路径分组、
/// 导入按 tag 建模块），所以比较的是集合而不是次序。
fn endpoint_signatures(ex: &Exchange) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();

    // 模块直接挂着的接口。
    for m in &ex.modules {
        for ep in &m.endpoints {
            out.push((ep.method.as_str().to_string(), ep.path.clone()));
        }
    }
    // 集合是另一层树，单独走。显式栈而非递归：导入侧的 folder 深度没有上限。
    let mut stack: Vec<&IrCollection> = ex.modules.iter().flat_map(|m| &m.collections).collect();
    while let Some(c) = stack.pop() {
        for ep in &c.endpoints {
            out.push((ep.method.as_str().to_string(), ep.path.clone()));
        }
        stack.extend(c.collections.iter().rev());
    }

    out.sort();
    out.dedup();
    out
}
