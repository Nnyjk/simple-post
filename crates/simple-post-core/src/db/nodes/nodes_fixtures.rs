//! 仅测试：`nodes` 各测试文件共用的树构造助手。
//!
//! 经公开 CRUD API 而非 `test_support::seed` 构造，故写路径与类型矩阵被直接考验。

use std::path::PathBuf;

use super::*;
use crate::db;
use crate::domain::{
    AuthConfig, EndpointDetail, Project, RequestBody, ResourceNode, ResourceNodeType,
};

/// 一个含两个项目、树内容为空的库。
/// 经公开 CRUD API 而非 `test_support::seed` 构造，故写路径与类型矩阵被直接考验。需要
/// 出厂树形状的测试改用 [`seeded_pool`]。
pub(crate) fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-nodes-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    for (id, name) in [("p1", "First"), ("p2", "Second")] {
        db::with_tx(&pool, |c| {
            db::projects::create(
                c,
                &Project {
                    id: id.into(),
                    name: name.into(),
                    description: None,
                    color: "#000000".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )
        })
        .unwrap();
    }
    (pool, dir)
}

pub(crate) fn node(
    root: &str,
    id: &str,
    kind: ResourceNodeType,
    parent: Option<&str>,
    sort: i64,
) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: root.into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: sort,
        created_at: 1,
        updated_at: 1,
    }
}

/// 一个顶层模块：其父是项目，而项目不是节点。
pub(crate) fn module(root: &str, id: &str, sort: i64) -> ResourceNode {
    node(root, id, ResourceNodeType::Module, None, sort)
}

pub(crate) fn collection(root: &str, id: &str, parent: &str, sort: i64) -> ResourceNode {
    node(root, id, ResourceNodeType::Collection, Some(parent), sort)
}

pub(crate) fn endpoint(
    root: &str,
    id: &str,
    parent: &str,
    sort: i64,
) -> (ResourceNode, EndpointDetail) {
    let d = detail(id);
    (
        node(root, id, ResourceNodeType::Endpoint, Some(parent), sort),
        d,
    )
}

pub(crate) fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: crate::domain::HttpMethod::Get,
        url: format!("/{node_id}"),
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
    }
}

pub(crate) fn put(pool: &db::DbPool, n: &ResourceNode, d: Option<&EndpointDetail>) {
    db::with_tx(pool, |c| create(c, n, d)).expect("create node");
}

/// 一个带着出厂 `fixtures.json` 树的库，含那个嵌套 collection（`c_order_refund`，
/// 挂在 `c_order_crud` 之下）。
/// 下面的测试需要一条真实的 `collection -> collection` 边，否则递归路径在测试数据里
/// 没有任何例子。
pub(crate) fn seeded_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-nodes-seeded-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    (db::test_support::init_seeded(&dir), dir)
}
