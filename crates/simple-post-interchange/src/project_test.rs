//! `domain → ir → domain` 往返与「不回退」的服务取值。

use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    uid, AuthConfig, AuthType, BaseUrlDefinition, BodyMode, EndpointDetail, HttpMethod, KeyValue,
    Project, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

use super::project_to_ir;
use crate::ir::{IrEndpoint, IrServer};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 一个项目：自带一个默认模块、一个激活环境，外加一条服务定义（名「网关」）。
struct Seeded {
    project_id: String,
    module_id: String,
    env_id: String,
    def_id: String,
}

fn temp_pool(tag: &str) -> DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-interchange-proj-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(&dir).expect("init")
}

/// `pool` 由调用方持有：多个项目必须落在**同一个**库里，否则「指名的环境属于别的项目」
/// 那条断言根本走不到拒绝那一步，就先在一个不同的文件上 NotFound 了。
fn seed_project(pool: &DbPool) -> Seeded {
    db::with_tx(pool, |conn| {
        let p = db::projects::create_with_defaults(
            conn,
            &Project {
                id: uid(),
                name: "导出项目".into(),
                description: None,
                base_url: None,
                base_url_definitions: Vec::new(),
                default_base_url_definition_id: None,
                color: "#3b82f6".into(),
                created_at: 0,
                updated_at: 0,
            },
        )?;
        // 服务表是项目级的，故这里显式建一行。id 每个项目各不相同：
        // `base_url_definitions.id` 是主键，同一个库里放两次同一个 id 会被拒。
        let def_id = format!("def_{}", p.id);
        db::base_url_defs::create(
            conn,
            &BaseUrlDefinition {
                id: def_id.clone(),
                name: "网关".into(),
                description: None,
            },
            &p.id,
        )?;
        let modules = db::nodes::list_by_root_and_type(conn, &p.id, ResourceNodeType::Module)?;
        let envs = db::environments::list_by_project(conn, &p.id)?;
        Ok::<Seeded, AppError>(Seeded {
            project_id: p.id,
            module_id: modules[0].id.clone(),
            env_id: envs[0].id.clone(),
            def_id,
        })
    })
    .expect("seed")
}

/// 一个请求配置齐备的 endpoint：往返要保住的就是这些字段。
fn rich_detail(node_id: String, def_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id,
        method: HttpMethod::Post,
        url: "/users/{{id}}".into(),
        environment_id: Some("e".into()),
        base_url_definition_id: Some(def_id.into()),
        base_url_override: None,
        params: vec![KeyValue {
            id: "p1".into(),
            key: "dry".into(),
            value: "false".into(),
            enabled: true,
            description: None,
        }],
        headers: vec![KeyValue {
            id: "h1".into(),
            key: "X-Trace".into(),
            value: "on".into(),
            enabled: true,
            description: None,
        }],
        body: RequestBody {
            mode: BodyMode::Json,
            content: "{\"a\":1}".into(),
            schema: Some("{\"limit\":\"number\"}".into()),
        },
        auth: AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some("tok".into()),
            ..Default::default()
        },
        docs: Some("doc".into()),
        notes: Some("note".into()),
        tags: vec!["用户".into()],
    }
}

fn add_endpoint(pool: &DbPool, s: &Seeded, parent: &str) -> String {
    db::with_tx(pool, |conn| {
        let node = ResourceNode {
            id: uid(),
            root_id: s.project_id.clone(),
            parent_id: Some(parent.to_string()),
            kind: ResourceNodeType::Endpoint,
            name: "往返接口".into(),
            description: Some("说明".into()),
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let id = node.id.clone();
        let detail = rich_detail(id.clone(), &s.def_id);
        db::nodes::create(conn, &node, Some(&detail))?;
        Ok::<String, AppError>(id)
    })
    .expect("create endpoint")
}

fn add_module(pool: &DbPool, project_id: &str, name: &str) -> String {
    db::with_tx(pool, |conn| {
        let node = ResourceNode {
            id: uid(),
            root_id: project_id.to_string(),
            parent_id: None,
            kind: ResourceNodeType::Module,
            name: name.into(),
            description: None,
            sort_order: 1,
            created_at: 0,
            updated_at: 0,
        };
        db::nodes::create(conn, &node, None)
    })
    .expect("create module")
    .id
}

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

// -------------------------------------------------------------------------
// module_to_ir —— 导出入口要的是「一个模块的子树」，不是整个项目。
// -------------------------------------------------------------------------

#[test]
fn module_to_ir_returns_only_that_module() {
    let pool = temp_pool("one-module");
    let s = seed_project(&pool);
    add_endpoint(&pool, &s, &s.module_id);
    // 另一个模块里也放一个接口，它不该出现在这一份 IR 里。
    let other = db::with_tx(&pool, |conn| {
        db::nodes::create(
            conn,
            &ResourceNode {
                id: uid(),
                root_id: s.project_id.clone(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "别的模块".into(),
                description: None,
                sort_order: 1,
                created_at: 0,
                updated_at: 0,
            },
            None,
        )
    })
    .expect("second module");
    add_endpoint(&pool, &s, &other.id);

    let ex = super::module_to_ir(&pool, &s.project_id, &s.module_id, Some(&s.env_id))
        .expect("module_to_ir");
    assert_eq!(ex.modules.len(), 1);
    assert_eq!(ex.modules[0].name, "默认模块");
    assert_eq!(ex.endpoint_count(), 1);
    assert_eq!(
        ex.environment_id.as_deref(),
        Some(s.env_id.as_str()),
        "环境 id 原样回显，前端据此确认用户选的那个真的被用上了"
    );
}

#[test]
fn module_to_ir_keys_are_node_ids() {
    // 导出侧的 `key` 必须是 `nodes.id`：勾选集要能换回具体是哪一行，而节点此刻
    // 已经存在了——这正是导入侧（源格式指针）与导出侧（节点 id）的分工。
    let pool = temp_pool("keys");
    let s = seed_project(&pool);
    let node_id = add_endpoint(&pool, &s, &s.module_id);

    let ex = super::module_to_ir(&pool, &s.project_id, &s.module_id, Some(&s.env_id))
        .expect("module_to_ir");
    assert_eq!(ex.modules[0].key, s.module_id);
    assert_eq!(ex.modules[0].endpoints[0].key, node_id);
    // 于是「按勾选裁剪」在导出侧可以直接用端点 key 去问
    // `project::module_to_ir` 之后那一棵 IR 的 `endpoint_keys`。
    assert_eq!(
        ex.endpoint_keys(),
        std::collections::BTreeSet::from([node_id]),
        "选择集与投影出的 key 必须是同一份"
    );
}

#[test]
fn module_to_ir_rejects_a_node_that_is_not_a_module() {
    let pool = temp_pool("not-module");
    let s = seed_project(&pool);
    let node_id = add_endpoint(&pool, &s, &s.module_id);

    let err =
        super::module_to_ir(&pool, &s.project_id, &node_id, None).expect_err("接口不是合法落点");
    assert!(
        matches!(&err, AppError::Validation(m) if m.contains("导出落点必须是模块")),
        "got: {err}"
    );
}

#[test]
fn module_to_ir_rejects_a_module_from_another_project() {
    let pool = temp_pool("cross-module");
    let a = seed_project(&pool);
    let b = seed_project(&pool);

    let err = super::module_to_ir(&pool, &b.project_id, &a.module_id, None)
        .expect_err("跨项目落点必须被拒");
    assert!(matches!(err, AppError::NotFound(_)), "got: {err}");
}
