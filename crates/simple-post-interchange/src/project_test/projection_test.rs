//! `domain → ir → domain` 往返、投影的幂等与「不回退」的服务取值。

use simple_post_core::db;

use super::project_to_ir;
use super::support::{
    add_endpoint, add_endpoint_without_detail, add_module, seed_project, temp_pool,
};
use crate::ir::IrEndpoint;

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
    // host 不在 endpoint 上：IR 存的是 path，地址另挂在模块那一格与端点自己身上。
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
fn the_address_is_read_per_module_with_no_fallback() {
    let pool = temp_pool("srv");
    let s = seed_project(&pool);
    add_module(&pool, &s.project_id, "没有地址的模块");

    db::with_tx(&pool, |conn| {
        db::environments::set_module_base_url(
            conn,
            &s.env_id,
            &s.module_id,
            Some("https://mod-a.example.com"),
            None,
        )
    })
    .expect("set url");

    let ex = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("project");

    let cell_of = |name: &str| -> Option<String> {
        ex.modules
            .iter()
            .find(|m| m.name == name)
            .and_then(|m| m.base_url.as_ref())
            .map(|c| c.url.clone())
    };
    assert_eq!(
        cell_of("默认模块").as_deref(),
        Some("https://mod-a.example.com")
    );
    assert!(
        cell_of("没有地址的模块").is_none(),
        "未配置的模块不得借用别的模块的地址"
    );
}

#[test]
fn an_environment_from_another_project_is_not_used() {
    let pool = temp_pool("cross");
    let a = seed_project(&pool);
    let b = seed_project(&pool);
    assert_ne!(a.project_id, b.project_id);

    // 指名别的项目的环境：地址会跨项目串过来，故当作「没选环境」。
    let ex = project_to_ir(&pool, &b.project_id, Some(&a.env_id)).expect("project");
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.base_url.is_none()));
}

#[test]
fn a_project_with_no_default_environment_projects_no_address() {
    let pool = temp_pool("noenv");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);

    // 删掉唯一的环境，再不指名任何环境：投影不猜，地址为空。
    db::with_tx(&pool, |conn| db::environments::delete(conn, &s.env_id)).expect("delete");

    let ex = project_to_ir(&pool, &s.project_id, None).expect("project");
    assert_eq!(ex.environment_id, None);
    assert!(ex.modules.iter().all(|m| m.base_url.is_none()));
    // 树本身照常投影出来：没有地址不影响接口清单。
    assert_eq!(ex.endpoint_count(), 1);
}

/// 两表不一致时导出要**出声**，不是把那个接口悄悄丢掉。
///
/// 节点在、detail 行不在，这个接口就打不出请求。导出侧要把这件事报成一条 Error：
/// 静默丢掉只会让用户看到「导出的文档里少了几个接口」，没有任何线索指向少了谁。
///
/// 诊断的指针是 `/nodes/<节点 id>`，用户据此能定位到是哪一个接口出的问题。
#[test]
fn an_endpoint_without_a_detail_row_is_reported_not_dropped_silently() {
    let pool = temp_pool("nodetail");
    let s = seed_project(&pool);
    let orphan = add_endpoint_without_detail(&pool, &s, &s.module_id);

    let ex = project_to_ir(&pool, &s.project_id, Some(&s.env_id)).expect("project");

    assert!(
        ex.modules
            .iter()
            .all(|m| m.endpoints.iter().all(|e| e.key != orphan)),
        "缺 detail 行的接口不该出现在导出结果里"
    );
    let errors: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::diagnostics::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", ex.diagnostics);
    assert_eq!(errors[0].pointer, format!("/nodes/{orphan}"));
}
