//! 路径参数声明与 operationId 的写法。

use serde_json::Value;
use simple_post_core::domain::{HttpMethod, KeyValue};

use super::support::{doc_of, ep, one_module};
use super::{to_document, DocFormat};
use crate::ir::IrEndpoint;

/// 一个只含一个端点的模块。诊断要单独看，故有的用例直接拿 `to_document` 的结果。
fn ex_with(endpoint: IrEndpoint) -> crate::ir::Exchange {
    one_module("m", vec![endpoint])
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
    //
    // 路径分隔符落成**双**下划线，于是路径的层级在名字里看得见。
    assert_eq!(
        doc["paths"]["/users/{id}"]["get"]["operationId"],
        "get___users___id_"
    );
    assert_eq!(
        doc["paths"]["/users/{id}"]["put"]["operationId"],
        "put___users___id_"
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

/// 两条不同的路径不能撞出同一个 `operationId`。
///
/// 规范要求 `operationId` 在文档内唯一，而「动词 + 路径」曾经把一切非法字符都换成
/// `_`，于是 `/api/user-info` 与 `/api/user/info` 都成了 `get__api_user_info`。撞名
/// 之后多数代码生成器会把 `operationId` 直接变成方法名，于是生成出两个同名方法。
///
/// 现有两层外部门禁都看不见这件事（`openapiv3` 校验的是模型，不校验唯一性；
/// Redocly 也没开那条规则），故这条测试就是那道门。
#[test]
fn two_paths_that_used_to_collapse_get_different_operation_ids() {
    let doc = doc_of(&one_module(
        "m",
        vec![
            ep("用户信息", HttpMethod::Get, "/api/user-info"),
            ep("用户信息（另一种写法）", HttpMethod::Get, "/api/user/info"),
        ],
    ));

    let a = doc["paths"]["/api/user-info"]["get"]["operationId"]
        .as_str()
        .expect("operationId");
    let b = doc["paths"]["/api/user/info"]["get"]["operationId"]
        .as_str()
        .expect("operationId");
    assert_ne!(a, b, "两条路径撞了同一个 operationId：{a}");
    // 路径分隔符要能**穿过**映射：撞名不靠事后编号解决——编号只是最后一道回退，那条路
    // 生成出来的名字看不出它对应哪条路径。
    //
    // 确切的值钉在这里：README 与文档注释里举的例子必须与它一致，抄错的名字会
    // 变成代码生成器里的方法名。
    assert_eq!(a, "get___api__user_info");
    assert_eq!(b, "get___api__user__info");
}

/// 映射不是单射时，后来的那个要编号，而不是撞名。
///
/// `/a-b` 与 `/a_b` 都是合法路径，但都映成 `get___a_b`。规范要的是「文档内唯一」，
/// 故后者拿到 `_2` 后缀。
#[test]
fn a_path_that_maps_to_the_same_name_gets_a_numbered_suffix() {
    let doc = doc_of(&one_module(
        "m",
        vec![
            ep("短横线", HttpMethod::Get, "/a-b"),
            ep("下划线", HttpMethod::Get, "/a_b"),
        ],
    ));

    let a = doc["paths"]["/a-b"]["get"]["operationId"]
        .as_str()
        .expect("operationId");
    let b = doc["paths"]["/a_b"]["get"]["operationId"]
        .as_str()
        .expect("operationId");
    assert_ne!(a, b, "撞名了：{a}");
    assert!(b.starts_with(a), "编号加在后一个上：{a} / {b}");
}

// -------------------------------------------------------------------------
// 请求头 —— 它们是 `in: header` 的参数条目
// -------------------------------------------------------------------------

fn header_of<'a>(doc: &'a Value, path: &str, verb: &str, name: &str) -> Option<&'a Value> {
    doc["paths"][path][verb]["parameters"]
        .as_array()?
        .iter()
        .find(|p| p["in"] == "header" && p["name"] == name)
}

/// 请求头要落在文档里。
///
/// 曾经 IR 里的 `headers` 被整个丢掉，且不出任何诊断——「导出再导入」这条回路上，
/// 所有请求头都没了，而用户看不到任何线索。OpenAPI 没有独立的请求头段落，头就是
/// `in: header` 的参数，导入侧正是按 `in: header` 映回 `headers`，故导出侧写
/// `in: header` 才接得上。
#[test]
fn an_enabled_request_header_becomes_a_header_parameter() {
    let ep = IrEndpoint {
        headers: vec![
            KeyValue {
                id: "h1".into(),
                key: "X-Trace".into(),
                value: "{{traceId}}".into(),
                enabled: true,
                description: None,
            },
            KeyValue {
                id: "h2".into(),
                key: "X-Off".into(),
                value: "1".into(),
                enabled: false,
                description: None,
            },
        ],
        ..ep("列表", HttpMethod::Get, "/users")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    let h = header_of(&doc, "/users", "get", "X-Trace").expect("X-Trace 要在文档里");
    assert_eq!(h["required"], true);
    assert_eq!(h["schema"]["type"], "string");
    // 禁用的头连请求都不会带，不该出现在文档里。
    assert!(
        header_of(&doc, "/users", "get", "X-Off").is_none(),
        "禁用的请求头不该被声明"
    );
}

/// 头的**值**不进文档。
///
/// 值里常有凭据与 `{{变量}}`，一份面向人的契约文档不该带它们；query 参数同样只写
/// 名字。头「在不在」由条目本身表达，值不表达。
#[test]
fn a_request_header_value_does_not_reach_the_document() {
    let ep = IrEndpoint {
        headers: vec![KeyValue {
            id: "h1".into(),
            key: "X-Trace".into(),
            value: "{{traceId}}".into(),
            enabled: true,
            description: None,
        }],
        ..ep("列表", HttpMethod::Get, "/users")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    assert!(
        !doc.to_string().contains("traceId"),
        "头的值不该出现在产物里：{doc}"
    );
}

/// 同名的头只能写一条，但被丢的那条要**说出来**。
///
/// OpenAPI 要求 `(name, in)` 唯一，重复的条目会让文档不合规；不说是静默丢弃。
#[test]
fn a_duplicated_request_header_is_written_once_and_reported() {
    let ep = IrEndpoint {
        headers: vec![
            KeyValue {
                id: "h1".into(),
                key: "X-Trace".into(),
                value: "1".into(),
                enabled: true,
                description: None,
            },
            KeyValue {
                id: "h2".into(),
                key: "X-Trace".into(),
                value: "2".into(),
                enabled: true,
                description: None,
            },
        ],
        ..ep("列表", HttpMethod::Get, "/users")
    };
    let out = to_document(&ex_with(ep), DocFormat::Json).expect("导出应成功");
    let doc: Value = serde_json::from_str(&out.content).expect("合法 JSON");

    let params = doc["paths"]["/users"]["get"]["parameters"]
        .as_array()
        .expect("声明了参数");
    assert_eq!(
        params.iter().filter(|p| p["in"] == "header").count(),
        1,
        "同名的头只写一条：{params:?}"
    );
    let infos: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::Severity::Info)
        .collect();
    assert_eq!(infos.len(), 1, "{:?}", out.diagnostics);
    assert_eq!(infos[0].dropped.as_deref(), Some("请求头 X-Trace"));
}

/// 一个端点上的头与参数**不**互相覆盖：它们是不同位置（`in` 不同）。
#[test]
fn a_header_and_a_query_parameter_may_share_a_name() {
    let ep = IrEndpoint {
        params: vec![KeyValue {
            id: "p1".into(),
            key: "trace".into(),
            value: "1".into(),
            enabled: true,
            description: None,
        }],
        headers: vec![KeyValue {
            id: "h1".into(),
            key: "trace".into(),
            value: "1".into(),
            enabled: true,
            description: None,
        }],
        ..ep("列表", HttpMethod::Get, "/users")
    };
    let doc = doc_of(&one_module("m", vec![ep]));

    assert!(param_of(&doc, "/users", "get", "trace").is_some());
    assert!(header_of(&doc, "/users", "get", "trace").is_some());
}
