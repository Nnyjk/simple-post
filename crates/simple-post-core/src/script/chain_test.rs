//! 脚本链装配的单元测试：顺序与「谁带脚本」。
//!
//! 顺序不能改：collection → 嵌套 collection → 端点，pre 与 test 同序。

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::send::ScriptOverride;
use super::*;
use crate::db;
use crate::db::nodes;
use crate::domain::{
    AuthConfig, EndpointDetail, HttpMethod, NodeScriptsPatch, Project, RequestBody, ResourceNode,
    ResourceNodeType,
};

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

fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Get,
        url: format!("/{node_id}"),
        environment_id: None,
        base_url_definition_id: None,
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

/// 一棵 `模块 → 外层 collection → 内层 collection → 端点` 的树。
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-script-chain-test-{}-{}",
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
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
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
            &node("c2", ResourceNodeType::Collection, Some("c1"), 0),
            None,
        )?;
        nodes::create(
            c,
            &node("e1", ResourceNodeType::Endpoint, Some("c2"), 0),
            Some(&detail("e1")),
        )?;
        Ok(())
    })
    .expect("seed tree");
    (pool, dir)
}

fn set_scripts(pool: &db::DbPool, node_id: &str, pre: &str, test: &str) {
    db::with_tx(pool, |c| {
        db::node_scripts::set(
            c,
            node_id,
            NodeScriptsPatch {
                pre_request_script: Some(Some(pre.to_string())),
                test_script: Some(Some(test.to_string())),
            },
        )
    })
    .expect("write scripts");
}

fn set_endpoint_scripts(pool: &db::DbPool, pre: &str, test: &str) {
    db::with_tx(pool, |c| {
        db::endpoint_details::update(
            c,
            "e1",
            crate::domain::EndpointDetailPatch {
                pre_request_script: Some(Some(pre.to_string())),
                test_script: Some(Some(test.to_string())),
                ..Default::default()
            },
        )
    })
    .expect("write endpoint scripts");
}

#[test]
fn the_chain_runs_outermost_collection_first_and_the_endpoint_last() {
    let (pool, _dir) = fresh_pool();
    set_scripts(&pool, "c1", "outer-pre", "outer-test");
    set_scripts(&pool, "c2", "inner-pre", "inner-test");
    set_endpoint_scripts(&pool, "endpoint-pre", "endpoint-test");

    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", None)).expect("chain");

    let labels: Vec<&str> = chain.iter().map(|u| u.source.label()).collect();
    assert_eq!(labels, vec!["c1", "c2", "e1"]);
    assert_eq!(chain[0].pre_request.as_deref(), Some("outer-pre"));
    assert_eq!(chain[1].pre_request.as_deref(), Some("inner-pre"));
    assert_eq!(chain[2].pre_request.as_deref(), Some("endpoint-pre"));
    // test 段与 pre 段同序。
    assert_eq!(chain[0].test.as_deref(), Some("outer-test"));
    assert_eq!(chain[2].test.as_deref(), Some("endpoint-test"));
}

#[test]
fn nodes_without_scripts_stay_out_of_the_chain() {
    let (pool, _dir) = fresh_pool();
    set_scripts(&pool, "c2", "inner-pre", "inner-test");

    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", None)).expect("chain");

    let labels: Vec<&str> = chain.iter().map(|u| u.source.label()).collect();
    assert_eq!(labels, vec!["c2"]);
}

#[test]
fn a_module_never_appears_in_the_chain() {
    let (pool, _dir) = fresh_pool();
    set_scripts(&pool, "c1", "pre", "test");

    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", None)).expect("chain");

    let labels: Vec<&str> = chain.iter().map(|u| u.source.label()).collect();
    assert_eq!(labels, vec!["c1"], "模块没有脚本，不该进链");
}

#[test]
fn an_endpoint_without_scripts_yields_no_chain() {
    let (pool, _dir) = fresh_pool();
    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", None)).expect("chain");
    assert!(chain.is_empty());
}

#[test]
fn the_override_replaces_the_endpoints_own_scripts() {
    let (pool, _dir) = fresh_pool();
    // 端点存的那份…
    db::with_tx(&pool, |c| {
        db::endpoint_details::update(
            c,
            "e1",
            crate::domain::EndpointDetailPatch {
                pre_request_script: Some(Some("stored".into())),
                test_script: Some(Some("stored-test".into())),
                ..Default::default()
            },
        )
    })
    .expect("store the endpoint scripts");

    // …被请求里带来的那份覆盖。
    let override_ = ScriptOverride {
        pre_request: Some("from the payload".into()),
        test: None,
    };
    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", Some(&override_))).expect("chain");

    let endpoint = chain.last().expect("端点在链的最后");
    assert_eq!(endpoint.pre_request.as_deref(), Some("from the payload"));
    // 覆盖**逐字段**生效：没提到的那个字段仍用存的那份。
    assert_eq!(endpoint.test.as_deref(), Some("stored-test"));
}

#[test]
fn an_empty_override_means_no_script() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        db::endpoint_details::update(
            c,
            "e1",
            crate::domain::EndpointDetailPatch {
                pre_request_script: Some(Some("stored".into())),
                test_script: Some(Some("stored-test".into())),
                ..Default::default()
            },
        )
    })
    .expect("store the endpoint scripts");

    // 空串 = 没有脚本（用户在界面上清掉了它）。
    let override_ = ScriptOverride {
        pre_request: Some(String::new()),
        test: Some(String::new()),
    };
    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", Some(&override_))).expect("chain");

    let endpoint = chain.last().expect("端点在链的最后");
    assert_eq!(endpoint.pre_request.as_deref(), Some(""));
    assert_eq!(endpoint.test.as_deref(), Some(""));
}

#[test]
fn a_draft_endpoint_still_runs_the_scripts_from_the_payload() {
    let (pool, _dir) = fresh_pool();
    // 端点没有 detail 行（草稿接口就是这个样子）。
    db::with_tx(&pool, |c| {
        c.execute("DELETE FROM endpoint_details WHERE node_id = 'e1'", [])
            .map_err(crate::error::AppError::Sqlite)?;
        Ok(())
    })
    .expect("drop the detail row");

    let override_ = ScriptOverride {
        pre_request: Some("draft script".into()),
        test: None,
    };
    let chain = db::with_conn(&pool, |c| collect_chain(c, "e1", Some(&override_))).expect("chain");

    let endpoint = chain.last().expect("端点在链的最后");
    assert_eq!(endpoint.pre_request.as_deref(), Some("draft script"));
}

#[test]
fn merging_environment_writes_sets_and_removes() {
    let mut variables = BTreeMap::from([("a".to_string(), "1".to_string())]);
    let writes = vec![
        EnvWrite {
            key: "b".to_string(),
            value: Some("2".to_string()),
        },
        EnvWrite {
            key: "a".to_string(),
            value: None,
        },
    ];

    merge_env(&mut variables, &writes);

    assert_eq!(variables.get("b").map(String::as_str), Some("2"));
    assert!(!variables.contains_key("a"));
}
