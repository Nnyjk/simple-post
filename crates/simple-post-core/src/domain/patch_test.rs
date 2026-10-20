// patch 结构体的三态：缺席 = 不碰该列，`null` = 清空，取值 = 写入。

use super::*;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn endpoint_patch_distinguishes_absent_from_null() {
    // `description` 字段缺席 → None
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "url": "/renamed"
    }))
    .unwrap();
    assert_eq!(p.url.as_deref(), Some("/renamed"));
    assert!(p.environment_id.is_none());

    // `environmentId: null` → Some(None)（清空该列）
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "environmentId": null
    }))
    .unwrap();
    assert_eq!(p.environment_id, Some(None));

    // `environmentId: "e1"` → Some(Some("e1"))
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "environmentId": "e1"
    }))
    .unwrap();
    assert_eq!(p.environment_id, Some(Some("e1".into())));
}

#[test]
fn node_patch_distinguishes_absent_from_null() {
    // `parentId` 缺席 → 节点原地不动
    let p: ResourceNodePatch = serde_json::from_value(json!({ "name": "renamed" })).unwrap();
    assert_eq!(p.name.as_deref(), Some("renamed"));
    assert!(p.parent_id.is_none());

    // `parentId: null` → 提升到项目顶层。那只对模块合法，但领域层承载的是意图，
    // 执行类型矩阵是 DB 层的事。
    let p: ResourceNodePatch = serde_json::from_value(json!({ "parentId": null })).unwrap();
    assert_eq!(p.parent_id, Some(None));
}

#[test]
fn a_default_patch_is_empty() {
    assert!(ResourceNodePatch::default().is_empty());
    assert!(EndpointDetailPatch::default().is_empty());
    assert!(ProjectPatch::default().is_empty());
    assert!(EnvironmentPatch::default().is_empty());
}

#[test]
fn every_patch_field_alone_makes_the_patch_non_empty() {
    // 这里守住的是：`is_empty` 曾是手写字段列表这个坑。加了字段却忘了扩列表，会让 MCP 的
    // `endpoint_update` 工具静默忽略该字段，而 Tauri 与 REST 路径——它们压根不调
    // `is_empty`——却会应用它。同一个 bug，随传输层不同给出两种相反的行为。
    // `is_empty` 现从序列化推导，故不会失配。
    // 本测试是第二道防线，而它真正的职责不同：加字段会逼编译器在这里补一行，把一次静默
    // 跳过变成一处忘不掉的编辑。刻意用结构体更新语法——它让字段名与断言同一行，好让评审
    // 一眼对照两边。

    let n = ResourceNodePatch {
        parent_id: Some(Some("c1".into())),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::parent_id");
    let n = ResourceNodePatch {
        name: Some("renamed".into()),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::name");
    let n = ResourceNodePatch {
        description: Some(Some("d".into())),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::description");
    let n = ResourceNodePatch {
        sort_order: Some(3),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::sort_order");

    let d = EndpointDetailPatch {
        method: Some(HttpMethod::Post),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::method");
    let d = EndpointDetailPatch {
        url: Some("/x".into()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::url");
    let d = EndpointDetailPatch {
        environment_id: Some(Some("e1".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::environment_id");
    let d = EndpointDetailPatch {
        base_url_definition_id: Some(Some("def1".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::base_url_definition_id");
    let d = EndpointDetailPatch {
        base_url_override: Some(Some("https://x".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::base_url_override");
    let d = EndpointDetailPatch {
        params: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::params");
    let d = EndpointDetailPatch {
        headers: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::headers");
    let d = EndpointDetailPatch {
        body: Some(RequestBody::default()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::body");
    let d = EndpointDetailPatch {
        auth: Some(AuthConfig::default()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::auth");
    let d = EndpointDetailPatch {
        docs: Some(Some("doc".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::docs");
    let d = EndpointDetailPatch {
        notes: Some(Some("note".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::notes");
    let d = EndpointDetailPatch {
        tags: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::tags");

    let p = ProjectPatch {
        name: Some("n".into()),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::name");
    let p = ProjectPatch {
        description: Some(Some("d".into())),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::description");
    let p = ProjectPatch {
        base_url: Some(Some("u".into())),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::base_url");
    let p = ProjectPatch {
        base_url_definitions: Some(vec![]),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::base_url_definitions");
    let p = ProjectPatch {
        default_base_url_definition_id: Some(Some("def1".into())),
        ..Default::default()
    };
    assert!(
        !p.is_empty(),
        "ProjectPatch::default_base_url_definition_id"
    );
    let p = ProjectPatch {
        color: Some("#fff".into()),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::color");

    let e = EnvironmentPatch {
        name: Some("n".into()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::name");
    let e = EnvironmentPatch {
        base_urls: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::base_urls");
    let e = EnvironmentPatch {
        variables: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::variables");
    let e = EnvironmentPatch {
        disabled_variables: Some(vec!["token".into()]),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::disabled_variables");
    let e = EnvironmentPatch {
        is_active: Some(false),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::is_active");
}

#[test]
fn clearing_a_nullable_column_is_not_an_empty_patch() {
    // `Some(None)` 意味着「写 NULL」而非「保持不动」——这正是 `double_option` 存在的理由。
    // 它绝不能读起来像一次空操作，否则「清空这个字段」就静默地不再清空了。
    let n = ResourceNodePatch {
        description: Some(None),
        ..Default::default()
    };
    assert!(!n.is_empty(), "clearing description is a real write");
    // ……且它确实发出了那个键。
    let v = serde_json::to_value(&n).unwrap();
    assert!(v.get("description").is_some(), "got {v}");
    assert_eq!(v["description"], serde_json::Value::Null);

    let d = EndpointDetailPatch {
        environment_id: Some(None),
        ..Default::default()
    };
    assert!(!d.is_empty(), "clearing environmentId is a real write");
    assert_eq!(
        serde_json::to_value(&d).unwrap()["environmentId"],
        serde_json::Value::Null
    );
}
