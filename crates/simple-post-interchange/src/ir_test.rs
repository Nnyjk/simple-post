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
