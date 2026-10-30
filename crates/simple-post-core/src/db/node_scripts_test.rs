use super::*;
use crate::db;
use crate::db::nodes;
use crate::domain::{
    AuthConfig, EndpointDetail, HttpMethod, NodeScriptsPatch, Project, RequestBody, ResourceNode,
    ResourceNodeType,
};
use std::path::PathBuf;

fn node(id: &str, kind: ResourceNodeType, parent: Option<&str>, sort: i64) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: "p1".into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: sort,
        created_at: 1,
        updated_at: 1,
    }
}

/// 一个含一个项目、一个模块、一个 collection、一个 endpoint 的库。
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-node-scripts-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        nodes::create(c, &node("m1", ResourceNodeType::Module, None, 0), None)?;
        nodes::create(
            c,
            &node("c1", ResourceNodeType::Collection, Some("m1"), 0),
            None,
        )?;
        nodes::create(
            c,
            &node("e1", ResourceNodeType::Endpoint, Some("c1"), 0),
            Some(&detail("e1")),
        )?;
        Ok(())
    })
    .expect("seed tree");
    (pool, dir)
}

fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Get,
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

fn patch(pre: Option<Option<String>>, test: Option<Option<String>>) -> NodeScriptsPatch {
    NodeScriptsPatch {
        pre_request_script: pre,
        test_script: test,
    }
}

#[test]
fn a_collection_with_no_scripts_reads_as_empty() {
    let (pool, _dir) = fresh_pool();
    let scripts = db::with_conn(&pool, |c| get(c, "c1")).expect("read");
    assert_eq!(scripts.node_id, "c1");
    assert!(scripts.pre_request_script.is_none());
    assert!(scripts.test_script.is_none());
}

#[test]
fn both_scripts_round_trip() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            "c1",
            patch(
                Some(Some("pm.request.url = 'x';".into())),
                Some(Some("pm.test('t', () => {});".into())),
            ),
        )
    })
    .expect("write");

    let scripts = db::with_conn(&pool, |c| get(c, "c1")).expect("read");
    assert_eq!(
        scripts.pre_request_script.as_deref(),
        Some("pm.request.url = 'x';")
    );
    assert_eq!(
        scripts.test_script.as_deref(),
        Some("pm.test('t', () => {});")
    );
}

#[test]
fn an_absent_key_leaves_the_other_script_alone() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            "c1",
            patch(Some(Some("pre".into())), Some(Some("test".into()))),
        )
    })
    .expect("write");
    db::with_tx(&pool, |c| {
        set(c, "c1", patch(Some(Some("pre2".into())), None))
    })
    .expect("patch");

    let scripts = db::with_conn(&pool, |c| get(c, "c1")).expect("read");
    assert_eq!(scripts.pre_request_script.as_deref(), Some("pre2"));
    assert_eq!(scripts.test_script.as_deref(), Some("test"));
}

#[test]
fn an_explicit_null_clears_one_script() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            "c1",
            patch(Some(Some("pre".into())), Some(Some("test".into()))),
        )
    })
    .expect("write");
    db::with_tx(&pool, |c| set(c, "c1", patch(Some(None), None))).expect("patch");

    let scripts = db::with_conn(&pool, |c| get(c, "c1")).expect("read");
    assert!(scripts.pre_request_script.is_none());
    assert_eq!(scripts.test_script.as_deref(), Some("test"));
}

#[test]
fn clearing_both_scripts_removes_the_row() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            "c1",
            patch(Some(Some("pre".into())), Some(Some("test".into()))),
        )
    })
    .expect("write");
    db::with_tx(&pool, |c| set(c, "c1", patch(Some(None), Some(None)))).expect("patch");

    let rows: i64 = db::with_conn(&pool, |c| {
        c.query_row("SELECT COUNT(*) FROM node_scripts", [], |row| row.get(0))
            .map_err(crate::error::AppError::Sqlite)
    })
    .expect("count");
    assert_eq!(rows, 0);
}

#[test]
fn a_module_cannot_carry_scripts() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| set(c, "m1", patch(Some(Some("x".into())), None)))
        .expect_err("模块不该能带脚本");
    assert!(matches!(err, crate::error::AppError::Validation(_)));
}

#[test]
fn the_database_also_refuses_scripts_on_a_non_collection() {
    let (pool, _dir) = fresh_pool();
    // 绕过应用层直接写库：触发器是第二道防线。
    let err = db::with_conn(&pool, |c| {
        c.execute(
            "INSERT INTO node_scripts (node_id, pre_request_script, test_script) \
             VALUES ('m1', 'x', NULL)",
            [],
        )
        .map_err(crate::error::AppError::Sqlite)
    })
    .expect_err("触发器应拒绝");
    assert!(err.to_string().contains("only collections carry scripts"));
}

#[test]
fn an_unknown_node_is_not_found() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| set(c, "nope", patch(None, None))).expect_err("未知节点");
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}
