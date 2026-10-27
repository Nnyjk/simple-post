//! IR 的树形计数与父子矩阵预检。

use simple_post_core::domain::{HttpMethod, ResourceNodeType};

use super::{may_contain, IrCollection, IrEndpoint, IrModule};
use crate::diagnostics::{Diagnostic, Severity};

fn ep(name: &str) -> IrEndpoint {
    IrEndpoint::new(name, HttpMethod::Get, format!("/{name}"))
}

#[test]
fn endpoint_count_sums_every_level() {
    // 顶层 1 个 + 中层 2 个 + 底层 1 个 + 另一个嵌套 collection 里的 3 个。
    let leaf = IrCollection {
        name: "leaf".into(),
        endpoints: vec![ep("d"), ep("e"), ep("f")],
        ..Default::default()
    };
    let mid = IrCollection {
        name: "mid".into(),
        endpoints: vec![ep("b"), ep("c")],
        collections: vec![leaf],
        ..Default::default()
    };
    let module = IrModule {
        name: "m".into(),
        endpoints: vec![ep("a")],
        collections: vec![mid],
        ..Default::default()
    };
    assert_eq!(module.endpoint_count(), 6);

    let ex = super::Exchange {
        modules: vec![
            module,
            IrModule {
                name: "m2".into(),
                endpoints: vec![ep("g")],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(ex.endpoint_count(), 7);
}

#[test]
fn placement_allows_the_documented_pairs() {
    use ResourceNodeType::*;
    for (parent, child) in [
        (Module, Collection),
        (Module, Endpoint),
        (Collection, Collection),
        (Collection, Endpoint),
    ] {
        assert_eq!(may_contain(parent, child), Ok(()), "{parent:?} → {child:?}");
    }
}

#[test]
fn placement_rejects_a_module_under_a_module() {
    use ResourceNodeType::*;
    let err = may_contain(Module, Module).unwrap_err();
    // 错误必须同时点名父子，否则用户看不出该改哪一处。
    assert_eq!(err, "a module cannot sit under a module");
}

#[test]
fn ir_endpoint_defaults_match_a_hand_built_one() {
    let e = IrEndpoint::new("x", HttpMethod::Post, "/x");
    assert_eq!(e.name, "x");
    assert_eq!(e.path, "/x");
    assert!(e.params.is_empty());
    assert!(e.headers.is_empty());
    assert!(e.tags.is_empty());
    assert!(e.description.is_none());
}

#[test]
fn diagnostics_filter_by_severity() {
    let ex = super::Exchange {
        diagnostics: vec![
            Diagnostic::warn("/item/0", "少了点什么"),
            Diagnostic::error("/item/1", "整个条目跳过"),
        ],
        ..Default::default()
    };
    assert_eq!(ex.diagnostics_of(Severity::Warn).len(), 1);
    assert_eq!(ex.diagnostics_of(Severity::Error).len(), 1);
    assert_eq!(ex.diagnostics_of(Severity::Info).len(), 0);
}

#[test]
fn a_key_survives_the_serde_round_trip() {
    let mut e = ep("登录");
    e.key = "/item/0/item/1/request".into();
    let m = IrModule {
        key: "/item/0".into(),
        name: "用户".into(),
        endpoints: vec![e],
        ..Default::default()
    };
    let text = serde_json::to_string(&m).expect("ser");
    let back: IrModule = serde_json::from_str(&text).expect("de");
    assert_eq!(back.key, "/item/0");
    assert_eq!(back.endpoints[0].key, "/item/0/item/1/request");
}

#[test]
fn an_ir_without_keys_still_deserialises() {
    // `key` 可以缺席：任何一份不带 `key` 的 IR 仍必须读得进来——否则换过构建的
    // 桌面端连到一份老缓存上就会整份失败。
    let legacy = r#"{
      "projectName": "p",
      "modules": [
        { "name": "用户", "endpoints": [ { "name": "登录", "method": "POST", "path": "/login" } ] }
      ]
    }"#;
    let ex: super::Exchange = serde_json::from_str(legacy).expect("de");
    assert_eq!(ex.modules[0].key, "");
    assert_eq!(ex.modules[0].endpoints[0].key, "");
    assert_eq!(ex.endpoint_count(), 1);
}

#[test]
fn endpoint_keys_collects_every_level() {
    let mut leaf_a = ep("d");
    leaf_a.key = "d".into();
    let mut leaf_b = ep("e");
    leaf_b.key = "e".into();
    let mut leaf_c = ep("f");
    leaf_c.key = "f".into();
    let mut top = ep("a");
    top.key = "a".into();
    let mut mid_b = ep("b");
    mid_b.key = "b".into();

    let ex = super::Exchange {
        modules: vec![IrModule {
            key: "m".into(),
            name: "m".into(),
            endpoints: vec![top],
            collections: vec![
                IrCollection {
                    key: "c0".into(),
                    name: "mid".into(),
                    endpoints: vec![mid_b],
                    collections: vec![IrCollection {
                        key: "c0c0".into(),
                        name: "leaf".into(),
                        endpoints: vec![leaf_a, leaf_b, leaf_c],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                // 一个端点都没有的分组不贡献任何 key。
                IrCollection {
                    key: "c1".into(),
                    name: "空分组".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let all = ex.endpoint_keys();
    let keys: Vec<&str> = all.iter().map(String::as_str).collect();
    // BTreeSet 保证顺序稳定，便于断言也不依赖 HashMap 的随机化。
    assert_eq!(keys, vec!["a", "b", "d", "e", "f"]);
    assert_eq!(crate::ir::subtree_keys(&ex.modules[0]).len(), 5);
}
