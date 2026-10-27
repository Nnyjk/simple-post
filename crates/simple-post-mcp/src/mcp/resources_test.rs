//! MCP resources 的测试。
//!
//! 重点不在「能不能读出来」，而在**这份说明会不会过期**：它里面点名的每个工具都
//! 必须真的注册着，且它描述的每个参数都必须在那个工具的 schema 里。指南一旦与实现
//! 脱节，agent 会照着不存在的参数去调，然后花好几轮才发现。

use super::resources;
use crate::mcp::tool_table;

#[test]
fn the_resource_is_listed_with_its_uri() {
    let listed = resources::list();
    let uris = listed["resources"]
        .as_array()
        .expect("resources array")
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect::<Vec<_>>();
    assert!(
        uris.contains(&resources::SPEC_AUTHORING_URI),
        "指南必须在 resources/list 里出现：{uris:?}"
    );
}

#[test]
fn reading_an_unknown_uri_is_refused() {
    assert!(
        resources::read("simple-post://nope").is_none(),
        "不认识的 uri 必须返回 None，由调用方报「没有这个资源」"
    );
}

/// 指南里点名的每个工具都真的注册着。
///
/// 这条是这份指南的主要防线：工具改名了而指南没改，agent 就会照着旧名字去调。
#[test]
fn every_tool_the_guide_names_is_registered() {
    let text = resources::read(resources::SPEC_AUTHORING_URI).expect("the guide is readable")
        ["contents"][0]["text"]
        .as_str()
        .expect("text")
        .to_string();
    let guide: serde_json::Value = serde_json::from_str(&text).expect("the guide is JSON");

    let registered = tool_table().iter().map(|e| e.name).collect::<Vec<_>>();

    let mut named = Vec::new();
    for call in guide["calls"].as_array().expect("calls") {
        let tool = call["tool"].as_str().expect("tool name");
        assert!(
            registered.contains(&tool),
            "指南里点名了没有注册的工具 `{tool}`"
        );
        named.push(tool.to_string());
    }

    // 一次调用的形状也得在里面说清楚。
    assert!(!named.is_empty(), "指南至少要给出一次完整的调用顺序");
}

/// 指南里给出的参数名，在对应工具的 schema 里必须真的有这个属性。
///
/// 同样一条防线，但管的是参数：指南说 `endpoint_create` 收 `collection_path`，
/// 而 schema 里没有它，agent 照指南填就会拿到一个「不认识这个字段」的错误。
#[test]
fn every_argument_the_guide_shows_exists_in_that_tool_schema() {
    let text = resources::read(resources::SPEC_AUTHORING_URI).expect("the guide is readable")
        ["contents"][0]["text"]
        .as_str()
        .expect("text")
        .to_string();
    let guide: serde_json::Value = serde_json::from_str(&text).expect("the guide is JSON");

    // 端点模板里的键，对 `endpoint_create` 的 schema。
    let template = guide["endpointTemplate"]
        .as_object()
        .expect("endpointTemplate");
    let schema = tool_table()
        .iter()
        .find(|e| e.name == "endpoint_create")
        .map(|e| e.schema.clone())
        .expect("endpoint_create is registered");
    let props = schema["properties"].as_object().expect("properties");
    for key in template.keys() {
        assert!(
            props.contains_key(key),
            "指南里的 endpointTemplate 写了 `{key}`，而 endpoint_create 的 schema 里没有它"
        );
    }

    // 调用序列里的 arguments 键，同样要在那个工具的 schema 里。
    for call in guide["calls"].as_array().expect("calls") {
        let tool = call["tool"].as_str().expect("tool name");
        let args = call["arguments"].as_object().expect("arguments");
        let schema = tool_table()
            .iter()
            .find(|e| e.name == tool)
            .map(|e| e.schema.clone())
            .expect("registered");
        let props = schema["properties"].as_object().expect("properties");
        for key in args.keys() {
            assert!(
                props.contains_key(key),
                "指南里说 `{tool}` 收 `{key}`，而它的 schema 里没有这个属性"
            );
        }
    }
}

/// `fastPath` 那份规格模板里的每个键，都要在 `spec_upsert` 的 schema 里真的存在。
///
/// 递归检查：规格是嵌套的（`endpoint` / `environment` / `service` 各是一段），
/// 只查顶层会漏掉「`environment` 段里那个键其实不存在」这种错。
///
/// 遇到「自由形状的 map」（schema 节点只有 `additionalProperties`、没有
/// `properties`）就停在那儿：那里的键名是运行时的数据，不是参数名。
#[test]
fn every_key_of_the_spec_template_exists_in_the_spec_upsert_schema() {
    let text = resources::read(resources::SPEC_AUTHORING_URI).expect("the guide is readable")
        ["contents"][0]["text"]
        .as_str()
        .expect("text")
        .to_string();
    let guide: serde_json::Value = serde_json::from_str(&text).expect("the guide is JSON");

    let fast = &guide["fastPath"];
    let tool = fast["tool"].as_str().expect("tool name");
    assert_eq!(tool, "spec_upsert");
    let schema = tool_table()
        .iter()
        .find(|e| e.name == tool)
        .map(|e| e.schema.clone())
        .expect("spec_upsert is registered");

    let mut problems = Vec::new();
    check_keys(
        &schema,
        fast["arguments"].as_object().expect("arguments"),
        "",
        &mut problems,
    );
    assert!(
        problems.is_empty(),
        "指南里的规格模板与 {tool} 的 schema 对不上：{problems:?}"
    );
}

/// 递归核对一层：模板里的每个键都必须在 schema 的 `properties` 里，
/// 且当模板值是对象、schema 那一项也有 `properties` 时继续往下核对。
fn check_keys(
    schema: &serde_json::Value,
    template: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    problems: &mut Vec<String>,
) {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        // 这一层是自由形状的 map（`additionalProperties`）或根本不是对象，
        // 里面的键名不是参数名，核对到此为止。
        return;
    };
    for (key, value) in template {
        let at = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        let Some(prop) = props.get(key) else {
            problems.push(format!("{at} 不在 schema 里"));
            continue;
        };
        if let Some(sub) = value.as_object() {
            check_keys(prop, sub, &at, problems);
        }
    }
}
