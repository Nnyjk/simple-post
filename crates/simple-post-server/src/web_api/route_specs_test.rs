//! OpenAPI 表 ↔ axum 路由表的一一对应。
//!
//! 表（`web_api/spec/**`）与挂载（`web_api/router.rs`）是两份独立的手写清单：表里多一条
//! 就是「文档承诺了不存在的接口」，表里少一条就是「接口存在但没人知道」。这里用
//! `public_route_specs()`（表的**代码实体**，不是源码文本，故能拿方法/路径的最终值）与
//! `crate::registry_parity_tests::axum_routes()`（`router.rs` 的 `.route(` 集合）对账；
//! 差额只允许落在 `spec/mod.rs` 里那份「internal (not in OpenAPI v1)」清单上 —— 那份清单
//! 是文档的一部分，故用 `include_str!` 读它的文本，保证它写在文档里而不是脑子里的例外。
//!
//! 另一半（`scripts/dump-openapi.mjs` 在表为空时会静默输出空 YAML）不归这里管。

use std::collections::BTreeSet;

use super::{public_route_specs, RouteSpec};
use crate::registry_parity_tests::{axum_routes, normalize_path, Route};

/// `spec/mod.rs` 的文档里那份「internal」清单 —— 不进 OpenAPI v1，但确实挂在路由上。
const SPEC_MOD_RS: &str = include_str!("spec/mod.rs");

/// `.route(` 的路径模板与表里的路径都先归一（`{id}` → `{}`）再比。
fn route_of(spec: &RouteSpec) -> Route {
    (spec.method.to_uppercase(), normalize_path(spec.path))
}

fn table() -> BTreeSet<Route> {
    public_route_specs().iter().map(route_of).collect()
}

/// 从 `spec/mod.rs` 的文档注释里抽 `//   GET  /api/…` 这种行。
fn internal_routes() -> BTreeSet<Route> {
    const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
    SPEC_MOD_RS
        .lines()
        .filter_map(|line| {
            let body = line.trim().strip_prefix("//")?.trim();
            let mut words = body.split_whitespace();
            let method = words.next()?;
            let path = words.next()?;
            if !METHODS.contains(&method) || !path.starts_with('/') {
                return None;
            }
            Some((method.to_string(), normalize_path(path)))
        })
        .collect()
}

#[test]
fn every_documented_route_is_actually_mounted() {
    let mounted = axum_routes();
    let missing: Vec<String> = table()
        .into_iter()
        .filter(|route| !mounted.contains(route))
        .map(|(method, path)| format!("{method} {path}"))
        .collect();
    assert!(
        missing.is_empty(),
        "这些条目在 OpenAPI 表里、router.rs 却没有对应 .route(…)：{missing:?}"
    );
}

#[test]
fn the_table_has_no_duplicate_entries() {
    let specs = public_route_specs();
    let unique: BTreeSet<Route> = specs.iter().map(route_of).collect();
    assert_eq!(
        unique.len(),
        specs.len(),
        "OpenAPI 表里有重复的 (method, path)，导出时会写两遍"
    );
}

#[test]
fn every_api_route_is_either_documented_or_listed_as_internal() {
    let known: BTreeSet<Route> = table().union(&internal_routes()).cloned().collect();
    let undocumented: Vec<String> = axum_routes()
        .into_iter()
        .filter(|(_, path)| path.starts_with("/api/") || path == "/healthz")
        .filter(|route| !known.contains(route))
        .map(|(method, path)| format!("{method} {path}"))
        .collect();
    assert!(
        undocumented.is_empty(),
        "这些路由既不在 OpenAPI 表里、也不在 spec/mod.rs 的 internal 清单里：{undocumented:?}"
    );
}
