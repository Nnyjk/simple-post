// 资源树、项目、环境、权限与设置单例的线格式往返。

use super::*;
use serde_json::json;

#[test]
fn environment_module_base_urls_round_trip() {
    let env = Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        module_base_urls: [
            (
                "m_auth".to_string(),
                ModuleBaseUrl {
                    url: "http://auth".into(),
                    proxy: None,
                },
            ),
            (
                "m_order".to_string(),
                ModuleBaseUrl {
                    url: "http://order".into(),
                    proxy: Some("https://gw/order".into()),
                },
            ),
        ]
        .into_iter()
        .collect(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        disabled_variables: Vec::new(),
        is_active: true,
    };
    let s = serde_json::to_string(&env).unwrap();
    assert!(s.contains("\"moduleBaseUrls\""), "{s}");
    let back: Environment = serde_json::from_str(&s).unwrap();
    assert_eq!(
        back.module_base_urls.get("m_auth").unwrap().url,
        "http://auth"
    );
    assert!(back.module_base_urls.get("m_auth").unwrap().proxy.is_none());
    assert_eq!(
        back.module_base_urls.get("m_order").unwrap().url,
        "http://order"
    );
    assert_eq!(
        back.module_base_urls
            .get("m_order")
            .unwrap()
            .proxy
            .as_deref(),
        Some("https://gw/order")
    );
    assert_eq!(back.variables.get("token").unwrap(), "abc");
    assert!(back.is_active);
}

#[test]
fn environment_deserialises_without_module_base_urls() {
    // 值层出现之前写下的载荷必须仍能加载：该字段带 `#[serde(default)]` 正是为了让它到达时
    // 为空，而不是让整次读失败。
    let back: Environment = serde_json::from_value(json!({
        "id": "e1",
        "projectId": "p1",
        "name": "dev",
        "variables": {},
        "isActive": false,
    }))
    .unwrap();
    assert!(back.module_base_urls.is_empty());
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
    // 这里守住的是：线格式必须与 `as_str()` 一致，且同时匹配 `agent_permissions` 的 CHECK
    // 约束与 TS 联合类型。三者只要有一处不一致，权限值就有第三种拼法。
    // 往返测试抓不到它——serde 读得回自己编出来的东西——所以靠的是这条断言。
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

    // endpoint 是唯一命名*两种*父的行：它可以挂在 module 下，也可以挂在 collection 下，
    // 故「单个 endpoint、无文件夹」的最简情况不必先建一个用完即弃的 collection。
    assert!(Endpoint.may_under(Module));
    assert!(Endpoint.may_under(Collection));
    assert!(!Endpoint.may_under(Endpoint));
}
