//! `domain → ir → domain` 往返、投影的幂等与「不回退」的服务取值。

use simple_post_core::db;
use simple_post_core::error::AppError;

use super::project_to_ir;
use super::support::{add_endpoint, add_module, seed_project, temp_pool};
use crate::ir::{IrEndpoint, IrServer};

#[test]
fn domain_to_ir_to_domain_keeps_the_request() {
    let pool = temp_pool("rt");
    let s = seed_project(&pool);
    let node_id = add_endpoint(&pool, &s, &s.module_id);

    let ex = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("project");
    assert_eq!(ex.environment_id.as_deref(), Some(s.env_id.as_str()));

    let ir_ep: &IrEndpoint = ex.modules[0]
        .endpoints
        .iter()
        .find(|e| e.path == "/users/{{id}}")
        .expect("endpoint projected");
    // host 不在 endpoint 上：IR 存的是 path，服务 URL 另挂在模块上。
    assert!(!ir_ep.path.contains("example.com"));

    // ir → 落库 → 读回，与原始行逐字段比。目标项目在同一个库里。
    let target = seed_project(&pool);
    crate::commit::commit(
        &pool,
        &target.project_id,
        crate::commit::CommitTarget::ProjectRoot,
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect("recommit");

    let before = db::with_conn(&pool, |c| db::endpoint_details::get(c, &node_id)).expect("orig");
    let after = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &target.project_id)
    })
    .expect("read back");
    assert_eq!(after.len(), 1);
    let after = &after[0];

    assert_eq!(after.method, before.method);
    assert_eq!(after.url, before.url);
    assert_eq!(after.params, before.params);
    assert_eq!(after.headers, before.headers);
    assert_eq!(after.body, before.body);
    assert_eq!(after.auth, before.auth);
    assert_eq!(after.docs, before.docs);
    assert_eq!(after.notes, before.notes);
    assert_eq!(after.tags, before.tags);
}

#[test]
fn projecting_twice_gives_the_same_answer() {
    let pool = temp_pool("idem");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);

    let first = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("first");
    let second = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("second");

    assert_eq!(first.modules.len(), second.modules.len());
    assert_eq!(
        first.modules[0].endpoints[0].path,
        second.modules[0].endpoints[0].path
    );
    assert_eq!(first.modules[0].endpoints[0].path, "/users/{{id}}");
}

#[test]
fn servers_are_read_per_module_with_no_fallback() {
    let pool = temp_pool("srv");
    let s = seed_project(&pool);
    add_module(&pool, &s.project_id, "没有地址的模块");

    // 项目 ★ 指向这个服务：若投影回落到 ★，两个模块都会有 URL。
    db::with_tx(&pool, |conn| {
        db::base_url_defs::set_default(conn, &s.project_id, Some(&s.def_id))
    })
    .expect("star");
    db::with_tx(&pool, |conn| {
        db::environments::set_module_base_url(
            conn,
            &s.env_id,
            &s.def_id,
            &s.module_id,
            Some("https://mod-a.example.com"),
        )
    })
    .expect("set url");

    let ex = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("project");

    let servers_of = |name: &str| -> Vec<IrServer> {
        ex.modules
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.servers.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        servers_of("默认模块"),
        vec![IrServer {
            service: "网关".into(),
            url: "https://mod-a.example.com".into(),
        }]
    );
    assert!(
        servers_of("没有地址的模块").is_empty(),
        "未配置的模块不得借用别的模块的 URL，也不得落回项目 ★"
    );
}

#[test]
fn an_environment_from_another_project_is_not_used() {
    let pool = temp_pool("cross");
    let a = seed_project(&pool);
    let b = seed_project(&pool);
    assert_ne!(a.project_id, b.project_id);

    // 指名别的项目的环境：URL 会跨项目串过来，故当作「没选环境」。
    let ex = project_to_ir(&pool, &b.project_id, Some(&a.env_id)).expect("project");
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.servers.is_empty()));
}

#[test]
fn a_project_with_no_active_environment_projects_empty_servers() {
    let pool = temp_pool("noenv");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);

    // 停用唯一的环境，再不指名任何环境：投影不猜，servers 为空。
    db::with_tx(&pool, |conn| {
        conn.execute(
            "UPDATE environments SET is_active = 0 WHERE id = ?1",
            [&s.env_id],
        )?;
        Ok::<(), AppError>(())
    })
    .expect("deactivate");

    let ex = project_to_ir(&pool, &s.project_id, None).expect("project");
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.servers.is_empty()));
    // 树本身照常投影出来：没有 URL 不影响接口清单。
    assert_eq!(ex.endpoint_count(), 1);
}
