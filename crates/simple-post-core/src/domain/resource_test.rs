// 资源树、项目、环境、权限与设置单例的线格式往返。

use super::*;
use serde_json::json;

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
        disabled_variables: Vec::new(),
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
