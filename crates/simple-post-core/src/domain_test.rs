use super::*;
use serde_json::json;

#[test]
fn http_method_round_trips_known_verbs() {
    for v in [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Head,
        HttpMethod::Options,
    ] {
        let s = serde_json::to_string(&v).unwrap();
        let back: HttpMethod = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn http_method_serializes_uppercase_strings() {
    assert_eq!(serde_json::to_string(&HttpMethod::Get).unwrap(), "\"GET\"");
    assert_eq!(
        serde_json::to_string(&HttpMethod::Post).unwrap(),
        "\"POST\""
    );
}

#[test]
fn http_method_custom_verb_passes_through() {
    let raw = "\"PROPFIND\"";
    let m: HttpMethod = serde_json::from_str(raw).unwrap();
    assert_eq!(m.as_str(), "PROPFIND");
}

#[test]
fn apikey_location_field_round_trips_without_a_rename() {
    // TS 镜像读的是 `inLocation`。`in` 是 TypeScript 关键字，而 `in_` 不是解法——camelCase 会
    // 把它直接映射回 `in`。
    let v = json!({
        "kind": "apikey",
        "apikey": { "key": "X-API-Key", "value": "abc", "inLocation": "header" }
    });
    let auth: AuthConfig = serde_json::from_value(v).unwrap();
    let apikey = auth.apikey.clone().unwrap();
    assert_eq!(apikey.in_location, ApiKeyLocation::Header);
    // ……且发出的键正是镜像声明的那个词。
    let back = serde_json::to_value(&auth).unwrap();
    assert_eq!(back["kind"], json!("apikey"));
    assert_eq!(back["apikey"]["inLocation"], json!("header"));
}

#[test]
fn endpoint_camel_case_field_names() {
    // 结构那一半与配置那一半现在是分开的两个类型，故这里检查两者都能从各自 TS 镜像预期的
    // camelCase 线形状反序列化。
    let v = json!({
        "id": "e1",
        "rootId": "p1",
        "parentId": "c1",
        "kind": "endpoint",
        "name": "foo",
        "sortOrder": 0,
        "createdAt": 1,
        "updatedAt": 2,
    });
    let n: ResourceNode = serde_json::from_value(v).unwrap();
    assert_eq!(n.parent_id.as_deref(), Some("c1"));
    assert_eq!(n.root_id, "p1");
    assert_eq!(n.kind, ResourceNodeType::Endpoint);
    assert_eq!(n.created_at, 1);

    let v = json!({
        "nodeId": "e1",
        "method": "GET",
        "url": "/x",
        "params": [],
        "headers": [],
        "body": { "mode": "none", "content": "" },
        "auth": { "kind": "none" },
        "tags": [],
    });
    let d: EndpointDetail = serde_json::from_value(v).unwrap();
    assert_eq!(d.node_id, "e1");
    assert_eq!(d.method, HttpMethod::Get);
    assert_eq!(d.url, "/x");
}

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
fn environment_base_urls_round_trip() {
    let env = Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: [("def_default".to_string(), "http://x".to_string())]
            .into_iter()
            .collect(),
        module_base_urls: [(
            "def_default".to_string(),
            [
                ("m_auth".to_string(), "http://auth".to_string()),
                ("m_order".to_string(), "http://order".to_string()),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        is_active: true,
    };
    let s = serde_json::to_string(&env).unwrap();
    // 线格式同时给出两张 URL map，而已下线的那张保留它历史上的 `baseUrls` 拼法，好让旧客户端
    // 仍能解析。
    assert!(s.contains("\"moduleBaseUrls\""), "{s}");
    let back: Environment = serde_json::from_str(&s).unwrap();
    assert_eq!(back.base_urls.get("def_default").unwrap(), "http://x");
    assert_eq!(
        back.module_base_urls
            .get("def_default")
            .unwrap()
            .get("m_auth")
            .unwrap(),
        "http://auth"
    );
    assert_eq!(back.variables.get("token").unwrap(), "abc");
    assert!(back.is_active);
}

#[test]
fn environment_deserialises_without_module_base_urls() {
    // 值层迁移之前写下的载荷必须仍能加载：该字段带 `#[serde(default)]` 正是为了让它到达时
    // 为空，而不是让整次读失败。
    let back: Environment = serde_json::from_value(json!({
        "id": "e1",
        "projectId": "p1",
        "name": "dev",
        "baseUrls": { "def_default": "http://x" },
        "variables": {},
        "isActive": false,
    }))
    .unwrap();
    assert!(back.module_base_urls.is_empty());
    assert_eq!(back.base_urls.get("def_default").unwrap(), "http://x");
}

#[test]
fn permission_round_trip() {
    for p in [Permission::Read, Permission::ReadCall, Permission::Full] {
        let s = serde_json::to_string(&p).unwrap();
        let back: Permission = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }
    assert_eq!(Permission::parse("read"), Some(Permission::Read));
    assert_eq!(Permission::parse("read_call"), Some(Permission::ReadCall));
    assert_eq!(Permission::parse("full"), Some(Permission::Full));
    assert_eq!(Permission::parse("nope"), None);
}

#[test]
fn permission_serde_wire_matches_as_str() {
    // 这里守住的是：`rename_all = "lowercase"` 曾把 ReadCall 拼成 `"readcall"`，一种既不匹配
    // `as_str()`、也不匹配 `agent_permissions` CHECK 约束、更不匹配 TS 联合类型的第三种
    // 拼法。往返测试抓不到它——serde 读得回自己编出来的东西——所以靠的是这条断言。
    for p in [Permission::Read, Permission::ReadCall, Permission::Full] {
        let wire = serde_json::to_string(&p).unwrap();
        assert_eq!(
            wire,
            format!("\"{}\"", p.as_str()),
            "serde wire form must equal as_str()"
        );
        let back: Permission = serde_json::from_str(&wire).unwrap();
        assert_eq!(p, back);
    }
}

#[test]
fn http_method_has_one_parser_for_every_entry_point() {
    // 任何调用方给的小写都必须落到与 MCP 路径相同的存库动词上。派生的 `Deserialize` 会把
    // `"get"` 变成 `Custom("get")`，再原样小写往返出去并照此存库。
    for (json, want) in [
        ("\"get\"", HttpMethod::Get),
        ("\"PoSt\"", HttpMethod::Post),
        ("\"PROPFIND\"", HttpMethod::Custom("PROPFIND".into())),
        ("\"propfind\"", HttpMethod::Custom("PROPFIND".into())),
    ] {
        let m: HttpMethod = serde_json::from_str(json).unwrap();
        assert_eq!(m, want, "deserialising {json}");
        assert_eq!(m.as_str(), want.as_str());
        // 不管进来的是什么，出去的必是大写。
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            format!("\"{}\"", m.as_str())
        );
        // ……且 `parse_http_method` 与 serde 路径给出同一个答案。
        assert_eq!(parse_http_method(json.trim_matches('"')).unwrap(), want);
    }
}

#[test]
fn http_method_rejects_non_token_input() {
    for bad in ["", "   ", "GET POST", "GE\tT", "你好", "GET/1"] {
        assert!(
            parse_http_method(bad).is_err(),
            "{bad:?} should not parse as a method"
        );
        // 同样的拒绝也走 serde 路径——一个解析器，一个判决。
        let err = serde_json::from_str::<HttpMethod>(&format!("\"{bad}\""));
        assert!(err.is_err(), "serde should reject {bad:?}");
    }
}

#[test]
fn project_always_emits_default_base_url_definition_id() {
    // TS 镜像把它声明为必填但可空，故「无默认」必须以显式 null 到达，而不是一个缺席的键。
    let p = Project {
        id: "p".into(),
        name: "n".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![],
        default_base_url_definition_id: None,
        color: "#3b82f6".into(),
        created_at: 0,
        updated_at: 0,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert!(
        v.get("defaultBaseUrlDefinitionId").is_some(),
        "key must be present, got {v}"
    );
    assert_eq!(v["defaultBaseUrlDefinitionId"], serde_json::Value::Null);
    // ……而设了值仍能往返。
    let p = Project {
        default_base_url_definition_id: Some("def1".into()),
        ..p
    };
    let back: Project = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    assert_eq!(back.default_base_url_definition_id.as_deref(), Some("def1"));
}

#[test]
fn a_default_patch_is_empty() {
    assert!(ResourceNodePatch::default().is_empty());
    assert!(EndpointDetailPatch::default().is_empty());
    assert!(ProjectPatch::default().is_empty());
    assert!(EnvironmentPatch::default().is_empty());
}

#[test]
fn request_settings_reads_a_blob_written_by_an_older_build() {
    // 值以 JSON 存在 `preferences` 里，故下一个构建必须读得动上一个构建写下的东西。明天给
    // `RequestSettings` 加的字段不能弄坏在它之前存过行的每个安装——这正是容器级
    // `#[serde(default)]` 买下的东西，而这里就是它守护的情形。
    let older = json!({ "timeoutMs": 5000 });
    let s: RequestSettings = serde_json::from_value(older).unwrap();
    assert_eq!(s.timeout_ms, 5000, "stored value must win");
    assert_eq!(
        s.follow_redirects,
        RequestSettings::default().follow_redirects,
        "absent field falls back to its default"
    );
    // 空 blob 是退化情形：全是默认值，不是错误。
    let s: RequestSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(s.timeout_ms, 30_000);
    assert!(s.follow_redirects);
    assert_eq!(s.max_response_size_kb, 5120);
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

#[test]
fn may_under_is_the_whole_parent_child_matrix() {
    use ResourceNodeType::{Collection, Endpoint, Module};

    // 模块的父是项目，而项目不是节点——故没有任何节点类型接受一个模块，且 `db::nodes`
    // 把「顶层」写成 `parent_id = NULL`。
    for parent in [Module, Collection, Endpoint] {
        assert!(
            !Module.may_under(parent),
            "a module must not sit under a {}",
            parent.as_str()
        );
    }

    assert!(Collection.may_under(Module));
    assert!(Collection.may_under(Collection));
    assert!(!Collection.may_under(Endpoint));

    // endpoint 是唯一命名*两种*父的行。强制一个 collection 曾让「单个 endpoint、无文件夹」
    // 这种最简情况先得建一个用完即弃的 collection。
    assert!(Endpoint.may_under(Module));
    assert!(Endpoint.may_under(Collection));
    assert!(!Endpoint.may_under(Endpoint));
}
