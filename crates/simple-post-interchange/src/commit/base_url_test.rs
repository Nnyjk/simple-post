//! 地址：外来文档的 host → 环境的 (环境 × 模块) 那一格。

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{uid, HttpMethod};
use simple_post_core::error::AppError;

use super::support::{fresh_project, seeded_module};
use super::{commit, CommitReport, CommitTarget};
use crate::diagnostics::Severity;
use crate::ir::{Exchange, IrBaseUrl, IrEndpoint, IrModule};

/// 一个带地址的模块，底下挂一个端点。
fn module_with_cell(key: &str, name: &str, url: &str, endpoint_key: &str, path: &str) -> IrModule {
    IrModule {
        key: key.into(),
        name: name.into(),
        base_url: Some(IrBaseUrl {
            url: url.into(),
            proxy: None,
        }),
        endpoints: vec![IrEndpoint {
            key: endpoint_key.into(),
            ..IrEndpoint::new("接口", HttpMethod::Get, path)
        }],
        ..Default::default()
    }
}

/// 一个模块，底下的端点带着自己的地址。
fn module_with_endpoint_override(
    key: &str,
    name: &str,
    url: &str,
    endpoint_key: &str,
    own: &str,
) -> IrModule {
    IrModule {
        key: key.into(),
        name: name.into(),
        base_url: Some(IrBaseUrl {
            url: url.into(),
            proxy: None,
        }),
        endpoints: vec![IrEndpoint {
            key: endpoint_key.into(),
            base_url_override: Some(own.into()),
            ..IrEndpoint::new("接口", HttpMethod::Get, "/ping")
        }],
        ..Default::default()
    }
}

fn exchange(modules: Vec<IrModule>) -> Exchange {
    Exchange {
        project_name: "p".into(),
        modules,
        ..Default::default()
    }
}

/// 落进项目顶层，勾满。
fn commit_to_root(
    pool: &DbPool,
    project_id: &str,
    ex: &Exchange,
    apply_base_url: bool,
) -> Result<CommitReport, AppError> {
    commit(
        pool,
        project_id,
        CommitTarget::ProjectRoot,
        ex,
        &ex.endpoint_keys(),
        apply_base_url,
    )
}

/// 落进一个已有的模块，勾满。
fn commit_into_module(
    pool: &DbPool,
    project_id: &str,
    module_id: &str,
    ex: &Exchange,
    apply_base_url: bool,
) -> Result<CommitReport, AppError> {
    commit(
        pool,
        project_id,
        CommitTarget::Module { module_id },
        ex,
        &ex.endpoint_keys(),
        apply_base_url,
    )
}

/// 项目当前激活环境的 id。
fn active_env_id(pool: &DbPool, project_id: &str) -> String {
    db::with_conn(pool, |c| db::environments::get_active(c, project_id))
        .expect("active env")
        .expect("项目有激活环境")
        .id
}

/// 某模块在给定环境里那一格的地址。
fn cell_of(pool: &DbPool, env_id: &str, module_id: &str) -> Option<String> {
    db::with_conn(pool, |c| db::environments::get(c, env_id))
        .expect("env")
        .module_base_urls
        .get(module_id)
        .map(|c| c.url.clone())
}

/// 项目里全部模块节点的 id。
fn module_ids(pool: &DbPool, project_id: &str) -> Vec<String> {
    db::with_conn(pool, |c| db::nodes::list_by_root(c, project_id))
        .expect("nodes")
        .into_iter()
        .filter(|n| n.kind == simple_post_core::domain::ResourceNodeType::Module)
        .map(|n| n.id)
        .collect()
}

/// 端点自己的地址。
fn override_of(pool: &DbPool, endpoint_id: &str) -> Option<String> {
    db::with_conn(pool, |c| db::endpoint_details::get(c, endpoint_id))
        .expect("detail")
        .base_url_override
}

#[test]
fn one_address_lands_in_the_module_cell_and_the_endpoint_follows_it() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![module_with_cell(
        "/item/0",
        "订单",
        "https://api.example.com",
        "/item/0/request/0",
        "/orders",
    )]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");

    assert_eq!(report.created_endpoint_ids.len(), 1);
    assert_eq!(report.base_urls.len(), 1, "一个模块写进一格");
    assert_eq!(report.base_urls[0].url, "https://api.example.com");
    assert!(report.base_urls[0].environment_name.is_some());

    let env_id = active_env_id(&pool, &project_id);
    let written_module = report.base_urls[0].module_id.clone();
    assert!(
        module_ids(&pool, &project_id).contains(&written_module),
        "写进的是这次新建的模块"
    );
    assert_eq!(
        cell_of(&pool, &env_id, &written_module).as_deref(),
        Some("https://api.example.com")
    );
    // 端点跟模块走，故不带自己的地址。
    assert_eq!(override_of(&pool, &report.created_endpoint_ids[0]), None);
}

#[test]
fn each_module_keeps_its_own_address() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![
        module_with_cell(
            "/item/0",
            "订单",
            "https://a.example.com",
            "/item/0/request/0",
            "/orders",
        ),
        module_with_cell(
            "/item/1",
            "用户",
            "https://b.example.com",
            "/item/1/request/0",
            "/users",
        ),
    ]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");
    assert_eq!(report.base_urls.len(), 2, "两个模块各写一格");

    let env_id = active_env_id(&pool, &project_id);
    let mut urls: Vec<String> = report
        .base_urls
        .iter()
        .map(|b| cell_of(&pool, &env_id, &b.module_id).unwrap_or_default())
        .collect();
    urls.sort();
    assert_eq!(urls, vec!["https://a.example.com", "https://b.example.com"]);
}

#[test]
fn an_endpoint_keeps_its_own_address_when_it_differs_from_the_module_cell() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![module_with_endpoint_override(
        "/item/0",
        "订单",
        "https://api.example.com",
        "/item/0/request/0",
        "https://other.example.com",
    )]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");
    assert_eq!(
        override_of(&pool, &report.created_endpoint_ids[0]).as_deref(),
        Some("https://other.example.com"),
        "端点与模块格子不同就带自己的地址"
    );
}

#[test]
fn importing_into_an_existing_module_gives_that_module_the_first_address() {
    let (pool, project_id) = fresh_project();
    let module_id = seeded_module(&pool, &project_id);
    let ex = exchange(vec![module_with_cell(
        "/item/0",
        "订单",
        "https://api.example.com",
        "/item/0/request/0",
        "/orders",
    )]);

    let report = commit_into_module(&pool, &project_id, &module_id, &ex, true).expect("commit");
    assert_eq!(report.base_urls.len(), 1);
    assert_eq!(report.base_urls[0].module_id, module_id);

    let env_id = active_env_id(&pool, &project_id);
    assert_eq!(
        cell_of(&pool, &env_id, &module_id).as_deref(),
        Some("https://api.example.com")
    );
}

#[test]
fn turning_the_address_off_leaves_the_cell_and_the_endpoint_alone() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![module_with_endpoint_override(
        "/item/0",
        "订单",
        "https://api.example.com",
        "/item/0/request/0",
        "https://other.example.com",
    )]);

    let report = commit_to_root(&pool, &project_id, &ex, false).expect("commit");
    assert!(report.base_urls.is_empty(), "关掉就不写格子");
    assert_eq!(
        override_of(&pool, &report.created_endpoint_ids[0]),
        None,
        "关掉时端点也不带地址"
    );

    let env_id = active_env_id(&pool, &project_id);
    for m in module_ids(&pool, &project_id) {
        assert_eq!(cell_of(&pool, &env_id, &m), None);
    }
}

#[test]
fn without_an_active_environment_the_address_has_nowhere_to_go() {
    let (pool, project_id) = fresh_project();
    // 把项目的激活环境撤掉。
    let env_id = active_env_id(&pool, &project_id);
    db::with_tx(&pool, |c| {
        c.execute(
            "UPDATE environments SET is_active = 0 WHERE id = ?1",
            [&env_id],
        )?;
        Ok::<(), AppError>(())
    })
    .expect("clear active");

    let ex = exchange(vec![module_with_cell(
        "/item/0",
        "订单",
        "https://api.example.com",
        "/item/0/request/0",
        "/orders",
    )]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");
    assert_eq!(report.base_urls.len(), 1);
    assert!(
        report.base_urls[0].environment_name.is_none(),
        "没有激活环境时地址没落进任何一格"
    );
    assert_eq!(report.base_urls[0].url, "https://api.example.com");

    let env = db::with_conn(&pool, |c| db::environments::get(c, &env_id)).expect("env");
    assert!(
        env.module_base_urls.is_empty(),
        "那一格仍然是空的：没有环境就没有地方放"
    );
}

#[test]
fn several_addresses_are_reported_without_blocking_the_import() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![
        module_with_cell(
            "/item/0",
            "订单",
            "https://a.example.com",
            "/item/0/request/0",
            "/orders",
        ),
        module_with_cell(
            "/item/1",
            "用户",
            "https://b.example.com",
            "/item/1/request/0",
            "/users",
        ),
    ]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");
    assert_eq!(report.created_endpoint_ids.len(), 2, "两个模块都落库");
    let info = report
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Info)
        .expect("多个地址要出声");
    let dropped = info.dropped.as_deref().unwrap_or_default();
    assert!(
        dropped.contains("https://a.example.com") && dropped.contains("https://b.example.com"),
        "说明要点名两台机器: {info:?}"
    );
}

#[test]
fn a_module_without_an_address_writes_no_cell() {
    let (pool, project_id) = fresh_project();
    let ex = exchange(vec![IrModule {
        key: "/item/0".into(),
        name: "订单".into(),
        endpoints: vec![IrEndpoint::new("接口", HttpMethod::Get, "/ping")],
        ..Default::default()
    }]);

    let report = commit_to_root(&pool, &project_id, &ex, true).expect("commit");
    assert!(report.base_urls.is_empty(), "没有地址就没有格子可写");
    assert_eq!(report.created_endpoint_ids.len(), 1, "接口照样落库");
}

#[test]
fn the_project_created_here_keeps_the_default_environment() {
    // 建项目的动作会种下一个激活环境——地址才有地方落。
    let (pool, project_id) = fresh_project();
    let env_id = active_env_id(&pool, &project_id);
    assert!(!env_id.is_empty());
    assert_ne!(env_id, uid());
}
