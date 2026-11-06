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
    assert!(p.base_url_override.is_none());

    // `baseUrlOverride: null` → Some(None)（清空该列）
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "baseUrlOverride": null
    }))
    .unwrap();
    assert_eq!(p.base_url_override, Some(None));

    // `baseUrlOverride: "https://e1"` → Some(Some("https://e1"))
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "baseUrlOverride": "https://e1"
    }))
    .unwrap();
    assert_eq!(p.base_url_override, Some(Some("https://e1".into())));
}

#[test]
fn the_path_params_patch_distinguishes_absent_from_null() {
    // `pathParams` 缺席 → 该列不动。
    let p: EndpointDetailPatch = serde_json::from_value(json!({ "url": "/renamed" })).unwrap();
    assert!(p.path_params.is_none());

    // `pathParams: null` → Some(None)：清空这一列。内建的 `Option<Vec<KeyValue>>` 把
    // 缺席与 `null` 折叠成同一件事，于是清空请求被当成「别动」而静默失效。
    let p: EndpointDetailPatch = serde_json::from_value(json!({ "pathParams": null })).unwrap();
    assert_eq!(p.path_params, Some(None));

    // `pathParams: []` → Some(Some([]))：写入空列表。
    let p: EndpointDetailPatch = serde_json::from_value(json!({ "pathParams": [] })).unwrap();
    assert_eq!(p.path_params, Some(Some(vec![])));
}

#[test]
fn node_patch_distinguishes_absent_from_null() {
    // `parentId` 缺席 → 节点原地不动
    let p: ResourceNodePatch = serde_json::from_value(json!({ "name": "renamed" })).unwrap();
    assert_eq!(p.name.as_deref(), Some("renamed"));
    assert!(p.parent_id.is_none());

    // `parentId: null` → 提升到项目顶层。那只对模块合法，但领域层接受的是意图，
    // 执行类型矩阵是 DB 层的事。
    let p: ResourceNodePatch = serde_json::from_value(json!({ "parentId": null })).unwrap();
    assert_eq!(p.parent_id, Some(None));
}

#[test]
fn a_patch_with_an_unknown_field_is_rejected() {
    // 字段名写错必须报错：serde 静默丢弃未知键的话，调用方以为改成功了，
    // 而库里的行一字未动。
    let err = serde_json::from_value::<ResourceNodePatch>(json!({ "sort_order": 5 })).unwrap_err();
    assert!(err.to_string().contains("sort_order"), "got {err}");

    let err =
        serde_json::from_value::<EndpointDetailPatch>(json!({ "baseUrlOveride": "https://e1" }))
            .unwrap_err();
    assert!(err.to_string().contains("baseUrlOveride"), "got {err}");
}

#[test]
fn a_resource_read_back_can_be_written_back_as_a_patch() {
    // 「取回再改再写回」是公开契约：客户端 GET 一个节点或一份端点配置，改一处，再把
    // 整个 body PATCH 回去。补丁类型带 `deny_unknown_fields`，故它必须认得资源自己的
    // 每一个字段名，否则这条往返从一个 400 开始。
    let node = ResourceNode {
        id: "n1".into(),
        root_id: "p1".into(),
        parent_id: Some("c1".into()),
        kind: ResourceNodeType::Endpoint,
        name: "查订单".into(),
        description: Some("说明".into()),
        sort_order: 3,
        created_at: 11,
        updated_at: 22,
    };
    let patch: ResourceNodePatch =
        serde_json::from_value(serde_json::to_value(&node).unwrap()).unwrap();
    assert_eq!(patch.name.as_deref(), Some("查订单"));
    assert_eq!(patch.sort_order, Some(3));

    let detail = EndpointDetail {
        node_id: "n1".into(),
        method: HttpMethod::Post,
        url: "/api/orders".into(),
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
    };
    let patch: EndpointDetailPatch =
        serde_json::from_value(serde_json::to_value(&detail).unwrap()).unwrap();
    assert_eq!(patch.url.as_deref(), Some("/api/orders"));
    assert!(!patch.is_empty(), "url 是一个真实的写入目标");
}

#[test]
fn a_patch_carrying_only_the_resource_identity_is_still_empty() {
    // 身份字段不指向任何可写列，故它们不构成一次写入：`is_empty` 仍为真，写路径不会
    // 因为它们多跑一趟 UPDATE，MCP 侧也不会把这次调用当成「改了东西」。
    let n: ResourceNodePatch = serde_json::from_value(json!({
        "id": "n1",
        "rootId": "p1",
        "kind": "endpoint",
        "createdAt": 11,
        "updatedAt": 22
    }))
    .unwrap();
    assert!(n.is_empty(), "身份字段不构成一次写入");
    let d: EndpointDetailPatch = serde_json::from_value(json!({ "nodeId": "n1" })).unwrap();
    assert!(d.is_empty(), "nodeId 不构成一次写入");
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
    // 这里守住的是：`is_empty` 从序列化推导，故新增字段不会与手写字段表失配。若改成手写
    // 字段列表，加了字段却忘了扩列表，会让 MCP 的
    // `endpoint_update` 工具静默忽略该字段，而 Tauri 与 REST 路径——它们压根不调
    // `is_empty`——却会应用它。同一个 bug，随传输层不同给出两种相反的行为。
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
        path_params: Some(Some(vec![])),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::path_params");
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
        module_base_urls: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::module_base_urls");
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
        name: Some("staging".into()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::name");
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
        base_url_override: Some(None),
        ..Default::default()
    };
    assert!(!d.is_empty(), "clearing baseUrlOverride is a real write");
    assert_eq!(
        serde_json::to_value(&d).unwrap()["baseUrlOverride"],
        serde_json::Value::Null
    );
}
