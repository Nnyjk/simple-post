//! 脚本的落库：端点的两段进 `endpoint_details`，集合的两段进 `node_scripts`，
//! 而**模块**存不了脚本——那种落点要说出来。

use simple_post_core::db;
use simple_post_core::domain::{HttpMethod, ResourceNodeType};

use super::support::{commit_all, fresh_project, seeded_module};
use super::{commit, CommitTarget};
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrScripts};

/// 一棵带脚本的树：一个模块（带脚本）、一个集合（带脚本）、一个接口（带脚本）。
fn exchange_with_scripts() -> Exchange {
    Exchange {
        project_name: "p".into(),
        modules: vec![IrModule {
            key: "/item/0".into(),
            name: "用户".into(),
            scripts: IrScripts {
                pre_request_script: Some("模块前置".into()),
                test_script: None,
            },
            collections: vec![IrCollection {
                key: "/item/0/item/0".into(),
                name: "账号".into(),
                scripts: IrScripts {
                    pre_request_script: Some("集合前置".into()),
                    test_script: Some("集合测试".into()),
                },
                endpoints: vec![IrEndpoint {
                    key: "/item/0/item/0/request/0".into(),
                    scripts: IrScripts {
                        pre_request_script: Some("接口前置".into()),
                        test_script: Some("接口测试".into()),
                    },
                    ..IrEndpoint::new("登录", HttpMethod::Post, "/login")
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn an_endpoint_script_lands_in_its_detail_row() {
    let (pool, project_id) = fresh_project();
    let ex = exchange_with_scripts();
    let report = commit_all(&pool, &project_id, &ex).expect("commit");

    let detail = db::with_conn(&pool, |c| {
        db::endpoint_details::get(c, &report.created_endpoint_ids[0])
    })
    .expect("detail");
    assert_eq!(detail.pre_request_script.as_deref(), Some("接口前置"));
    assert_eq!(detail.test_script.as_deref(), Some("接口测试"));
}

#[test]
fn a_collection_script_lands_in_the_node_scripts_table() {
    let (pool, project_id) = fresh_project();
    let ex = exchange_with_scripts();
    commit_all(&pool, &project_id, &ex).expect("commit");

    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    let collection = nodes
        .iter()
        .find(|n| n.kind == ResourceNodeType::Collection && n.name == "账号")
        .expect("那个集合");
    let scripts =
        db::with_conn(&pool, |c| db::node_scripts::get(c, &collection.id)).expect("scripts");
    assert_eq!(scripts.pre_request_script.as_deref(), Some("集合前置"));
    assert_eq!(scripts.test_script.as_deref(), Some("集合测试"));
}

/// 模块**不能**存脚本：落到项目顶层时它只能是模块，那两段脚本无处可去——
/// 出诊断说出来，不静默丢。
#[test]
fn a_module_script_is_reported_when_the_group_lands_as_a_module() {
    let (pool, project_id) = fresh_project();
    let ex = exchange_with_scripts();
    let report = commit_all(&pool, &project_id, &ex).expect("commit");

    let warns: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("模块"))
        .collect();
    assert_eq!(warns.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(warns[0].pointer, "/item/0");
    assert_eq!(warns[0].severity, crate::Severity::Warn);

    // 模块节点确实没有脚本行。
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    let module = nodes
        .iter()
        .find(|n| n.kind == ResourceNodeType::Module && n.name == "用户")
        .expect("那个模块");
    let scripts = db::with_conn(&pool, |c| db::node_scripts::get(c, &module.id)).expect("scripts");
    assert!(scripts.pre_request_script.is_none());
}

/// 挂进**已有模块**时，IR 顶层的模块降级成集合，脚本就有归宿了。
#[test]
fn a_module_script_lands_when_the_group_becomes_a_collection() {
    let (pool, project_id) = fresh_project();
    let anchor = seeded_module(&pool, &project_id);
    let ex = exchange_with_scripts();

    let report = commit(
        &pool,
        &project_id,
        CommitTarget::Module { module_id: &anchor },
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect("commit");

    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("模块")),
        "降级成集合后脚本有归宿，不该报诊断：{:?}",
        report.diagnostics
    );
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    let collection = nodes
        .iter()
        .find(|n| n.kind == ResourceNodeType::Collection && n.name == "用户")
        .expect("那个集合");
    let scripts =
        db::with_conn(&pool, |c| db::node_scripts::get(c, &collection.id)).expect("scripts");
    assert_eq!(scripts.pre_request_script.as_deref(), Some("模块前置"));
}
