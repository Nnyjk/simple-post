//! 服务：host → `base_url_definitions` + `environments.module_base_urls`。

use std::sync::atomic::Ordering;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{uid, HttpMethod, Project, ResourceNode, ResourceNodeType};

use super::support::{fresh_project, seeded_module, COUNTER};
use super::{commit, plan_service, CommitReport, CommitTarget};
use crate::diagnostics::Severity;
use crate::ir::{Exchange, IrEndpoint, IrModule, IrServer};

/// 逐个模块给一组 `(服务名, URL)`。路径按下标走，故同一份交换里的端点不会互为重复。
fn exchange_with_hosts(hosts: &[(&str, &str)]) -> Exchange {
    Exchange {
        project_name: "p".into(),
        modules: hosts
            .iter()
            .enumerate()
            .map(|(i, (service, url))| IrModule {
                key: format!("/m/{i}"),
                name: format!("模块{i}"),
                servers: vec![IrServer {
                    service: (*service).into(),
                    url: (*url).into(),
                }],
                endpoints: vec![IrEndpoint {
                    key: format!("/m/{i}/0"),
                    ..IrEndpoint::new("直挂的接口", HttpMethod::Get, format!("/ping/{i}"))
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// 「勾满 + 挂进某个已有模块」。服务那几条测试要的是这个落点。
fn commit_into(
    pool: &DbPool,
    project_id: &str,
    module_id: &str,
    ex: &Exchange,
    apply_service: bool,
) -> CommitReport {
    commit(
        pool,
        project_id,
        CommitTarget::Module { module_id },
        ex,
        &ex.endpoint_keys(),
        apply_service,
    )
    .expect("commit")
}

/// 再加一个模块。URL 的住址是 (环境 × 模块 × 服务)，故跨模块隔离只有两个模块可谈。
fn add_module(pool: &DbPool, project_id: &str, name: &str) -> String {
    db::with_tx(pool, |conn| {
        let node = ResourceNode {
            id: uid(),
            root_id: project_id.to_string(),
            parent_id: None,
            kind: ResourceNodeType::Module,
            name: name.into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        Ok(db::nodes::create(conn, &node, None)?.id)
    })
    .expect("add module")
}

/// 读回那一个 URL 单元格。缺席是真实答案，故返回 `Option` 而不补默认值。
fn url_cell(pool: &DbPool, project_id: &str, def_id: &str, module_id: &str) -> Option<String> {
    db::with_conn(pool, |conn| {
        let env = db::environments::get_active(conn, project_id)?.expect("激活环境");
        Ok(env
            .module_base_urls
            .get(def_id)
            .and_then(|by_module| by_module.get(module_id))
            .cloned())
    })
    .expect("read cell")
}

fn service_ids(pool: &DbPool, project_id: &str) -> Vec<String> {
    db::with_conn(pool, |conn| {
        Ok(db::base_url_defs::list_by_project(conn, project_id)?
            .into_iter()
            .map(|d| d.id)
            .collect())
    })
    .expect("services")
}

fn active_env_id(pool: &DbPool, project_id: &str) -> String {
    db::with_conn(pool, |conn| {
        Ok(db::environments::get_active(conn, project_id)?
            .expect("激活环境")
            .id)
    })
    .expect("active env")
}

fn detail_of(pool: &DbPool, node_id: &str) -> simple_post_core::domain::EndpointDetail {
    db::with_conn(pool, |conn| {
        Ok(db::endpoint_details::get(conn, node_id).expect("detail"))
    })
    .expect("detail")
}

#[test]
fn a_single_host_becomes_a_service_and_the_url_lands_in_that_module() {
    let (pool, project_id) = fresh_project();
    let module_id = seeded_module(&pool, &project_id);
    let ex = exchange_with_hosts(&[("api.example.com", "https://api.example.com")]);

    let report = commit_into(&pool, &project_id, &module_id, &ex, true);

    let service = report.service.expect("恰好一个 host 就该落成服务");
    assert_eq!(service.name, "api.example.com");
    assert_eq!(service.url, "https://api.example.com");
    assert!(service.created, "项目里还没有这个服务");
    assert!(service.assigned, "项目有激活环境，格子与端点都有处可落");
    assert_eq!(
        service.environment_name.as_deref(),
        Some(db::projects::DEFAULT_ENVIRONMENT_NAME)
    );

    let ids = service_ids(&pool, &project_id);
    assert_eq!(ids.len(), 1, "服务表里只该多出这一台");
    let def_id = &ids[0];
    assert_eq!(
        url_cell(&pool, &project_id, def_id, &module_id).as_deref(),
        Some("https://api.example.com"),
        "URL 的唯一住址是 def_id × module_id 那一格"
    );

    // 端点必须指过去，否则 URL 解析链连那一格都到不了。
    let detail = detail_of(&pool, &report.created_endpoint_ids[0]);
    assert_eq!(
        detail.base_url_definition_id.as_deref(),
        Some(def_id.as_str())
    );
    assert_eq!(
        detail.environment_id.as_deref(),
        Some(active_env_id(&pool, &project_id).as_str())
    );
}

#[test]
fn the_same_host_in_several_modules_is_still_one_service() {
    // Postman 的每个 folder 都把 host 重写一遍。那是同一台服务，不是多台。
    let (pool, project_id) = fresh_project();
    let ex = exchange_with_hosts(&[
        ("api.example.com", "https://api.example.com"),
        ("api.example.com", "https://api.example.com"),
    ]);

    let plan = db::with_conn(&pool, |c| plan_service(c, &project_id, &ex)).expect("plan");
    assert_eq!(plan.candidates.len(), 1, "逐个 (服务名, URL) 去重");
    assert_eq!(
        plan.server.as_ref().map(|s| s.service.as_str()),
        Some("api.example.com")
    );
    assert!(plan.ambiguous_diagnostic().is_none());
}

#[test]
fn one_service_name_carrying_two_urls_counts_as_two_candidates() {
    // 去重的键是 (服务名, URL) 而不是服务名：服务表按名字唯一，同名挂两个地址落不进去。
    // 合成一台就等于替用户挑了一个协议，而且挑得毫无依据。
    let (pool, project_id) = fresh_project();
    let ex = exchange_with_hosts(&[
        ("api.example.com", "http://api.example.com"),
        ("api.example.com", "https://api.example.com"),
    ]);

    let plan = db::with_conn(&pool, |c| plan_service(c, &project_id, &ex)).expect("plan");
    assert_eq!(plan.candidates.len(), 2);
    assert!(plan.server.is_none(), "同名两地址不能合成一台");
    assert!(plan.ambiguous_diagnostic().is_some());
}

#[test]
fn two_hosts_are_refused_by_name_instead_of_one_being_picked() {
    let (pool, project_id) = fresh_project();
    let module_id = seeded_module(&pool, &project_id);
    let ex = exchange_with_hosts(&[
        ("a.example.com", "https://a.example.com"),
        ("b.example.com", "https://b.example.com"),
    ]);

    let report = commit_into(&pool, &project_id, &module_id, &ex, true);

    // 端点不带 host，IR 里也没有位置能写「这一个接口指哪台」，故不猜。
    assert!(report.service.is_none(), "多 host 不该凭空选一个");
    let warn = report
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Warn)
        .expect("多 host 必须出声");
    let dropped = warn.dropped.as_deref().unwrap_or_default();
    assert!(
        dropped.contains("a.example.com") && dropped.contains("b.example.com"),
        "诊断要点名两台，用户才能自己挑那一台: {warn:?}"
    );
    assert!(service_ids(&pool, &project_id).is_empty(), "一台都不该建");
}

#[test]
fn switching_the_service_off_leaves_the_service_table_and_the_endpoints_alone() {
    let (pool, project_id) = fresh_project();
    let module_id = seeded_module(&pool, &project_id);
    let ex = exchange_with_hosts(&[("api.example.com", "https://api.example.com")]);

    let report = commit_into(&pool, &project_id, &module_id, &ex, false);

    assert!(report.service.is_none());
    assert!(service_ids(&pool, &project_id).is_empty());
    let detail = detail_of(&pool, &report.created_endpoint_ids[0]);
    assert_eq!(
        detail.base_url_definition_id, None,
        "关掉服务时端点仍走项目的 ★，与这次导入之前的行为逐字节相同"
    );
}

#[test]
fn reusing_a_service_never_rewrites_another_modules_cell() {
    let (pool, project_id) = fresh_project();
    let first = seeded_module(&pool, &project_id);
    let second = add_module(&pool, &project_id, "第二个模块");
    let ex = exchange_with_hosts(&[("api.example.com", "https://api.example.com")]);

    commit_into(&pool, &project_id, &first, &ex, true);
    let def_id = service_ids(&pool, &project_id)[0].clone();
    let env_id = active_env_id(&pool, &project_id);

    // 用户把第一个模块的地址改成了预发。
    db::with_tx(&pool, |c| {
        db::environments::set_module_base_url(
            c,
            &env_id,
            &def_id,
            &first,
            Some("https://staging.example.com"),
        )?;
        Ok(())
    })
    .expect("改地址");

    let report = commit_into(&pool, &project_id, &second, &ex, true);

    let service = report.service.expect("第二次仍应有服务");
    assert!(!service.created, "同名服务应当复用，不该多出一台");
    assert_eq!(
        service_ids(&pool, &project_id),
        vec![def_id.clone()],
        "复用不是新建"
    );
    // 服务行本身不持有 URL，故填自己那一格动不了别的模块的地址 —— 这是复用安全的根据。
    assert_eq!(
        url_cell(&pool, &project_id, &def_id, &first).as_deref(),
        Some("https://staging.example.com"),
        "第一个模块的地址是用户自己改的，不能被这次导入抹掉"
    );
    assert_eq!(
        url_cell(&pool, &project_id, &def_id, &second).as_deref(),
        Some("https://api.example.com")
    );
}

#[test]
fn without_an_active_environment_the_service_exists_but_the_url_goes_nowhere() {
    // `create_with_defaults` 总会种一个激活环境出来，故这条路径得自己搭一个空项目。
    let dir = std::env::temp_dir().join(format!(
        "simple-post-interchange-nosvcenv-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    let (project_id, module_id) = db::with_tx(&pool, |conn| {
        let project = Project {
            id: uid(),
            name: "裸项目".into(),
            description: None,
            base_url: None,
            base_url_definitions: Vec::new(),
            default_base_url_definition_id: None,
            color: "#3b82f6".into(),
            created_at: 0,
            updated_at: 0,
        };
        db::projects::create(conn, &project)?;
        let node = ResourceNode {
            id: uid(),
            root_id: project.id.clone(),
            parent_id: None,
            kind: ResourceNodeType::Module,
            name: "模块".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        Ok((project.id, db::nodes::create(conn, &node, None)?.id))
    })
    .expect("裸项目");
    let ex = exchange_with_hosts(&[("api.example.com", "https://api.example.com")]);

    let report = commit_into(&pool, &project_id, &module_id, &ex, true);

    let service = report.service.expect("服务仍然要建");
    assert!(service.created);
    assert!(!service.assigned, "没有环境就没有格子，URL 无处可放");
    assert_eq!(service.environment_name, None);
    assert_eq!(service_ids(&pool, &project_id).len(), 1, "服务还是建了");
    let detail = detail_of(&pool, &report.created_endpoint_ids[0]);
    assert_eq!(detail.environment_id, None, "端点不能指向一个不存在的环境");
    assert_eq!(
        detail.base_url_definition_id, None,
        "没有环境就没有可解析的地址，指过去也解析不出来"
    );
}
