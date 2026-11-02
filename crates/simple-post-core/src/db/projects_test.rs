use super::*;
use crate::db;
use crate::domain::ResourceNodeType;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-projects-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

fn sample_project(id: &str, name: &str) -> Project {
    Project {
        id: id.to_string(),
        name: name.to_string(),
        description: Some("desc".into()),
        color: "#000".into(),
        created_at: 1000,
        updated_at: 2000,
    }
}

#[test]
fn create_and_get_round_trip() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "Project One"))?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert_eq!(got.name, "Project One");
    assert_eq!(got.color, "#000");
    assert_eq!(got.created_at, 1000);
}

#[test]
fn get_missing_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| get(c, "missing")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn list_returns_seeded_projects() {
    let pool = fresh_pool();
    let ps = db::with_conn(&pool, list).unwrap();
    // fixtures.json seeds 3 projects
    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0].id, "p_demo");
}

#[test]
fn update_partial_only_changes_provided_fields() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "Original"))?;
        update(
            c,
            "p1",
            ProjectPatch {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert_eq!(got.name, "Renamed");
    assert_eq!(got.description.as_deref(), Some("desc"));
    assert!(got.updated_at >= 2000, "updated_at should bump on patch");
}

#[test]
fn update_can_clear_a_nullable_field_via_double_none() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "x"))?;
        update(
            c,
            "p1",
            serde_json::from_str(r#"{"description": null}"#).unwrap(),
        )?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert!(got.description.is_none(), "explicit null clears the column");
}

#[test]
fn delete_removes_project_and_cascades_to_the_whole_tree() {
    let pool = fresh_pool();
    // 一个模块、一个嵌套 collection、其下的一个 endpoint，外加那个 endpoint 的配置行。
    // 删项目必须带走全部四个：`nodes.root_id` 从 `projects` 级联，`nodes.parent_id`
    // 沿树向下级联，`endpoint_details` 从它的节点级联。只断言模块的话，即便更深的层次泄漏
    // 也会通过。
    db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('m_extra', 'p_demo', NULL, 'module', 'extra', 99, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('c_extra', 'p_demo', 'm_extra', 'collection', 'c', 0, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('e_extra', 'p_demo', 'c_extra', 'endpoint', 'e', 0, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO endpoint_details (node_id, method, url) \
                 VALUES ('e_extra', 'GET', '/x')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // `endpoint_details` 以 `node_id` 为键，`nodes` 以 `id` 为键：两张表面对的是同一个问题
    // ——「这一行还在吗」。
    let count = |table: &str, col: &str, id: &str| {
        db::with_conn(&pool, |c| {
            let n: i64 = c.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {col} = ?1"),
                [id],
                |r| r.get(0),
            )?;
            Ok(n)
        })
        .unwrap()
    };
    let node = |id: &str| count("nodes", "id", id);
    let detail = |id: &str| count("endpoint_details", "node_id", id);

    assert_eq!(node("m_extra"), 1);
    assert_eq!(node("c_extra"), 1);
    assert_eq!(node("e_extra"), 1);
    assert_eq!(detail("e_extra"), 1);

    db::with_tx(&pool, |c| delete(c, "p_demo")).unwrap();

    assert_eq!(
        node("m_extra"),
        0,
        "ON DELETE CASCADE should drop child rows"
    );
    assert_eq!(node("c_extra"), 0);
    assert_eq!(node("e_extra"), 0);
    assert_eq!(
        detail("e_extra"),
        0,
        "the endpoint's configuration must not outlive its node"
    );
}

#[test]
fn delete_missing_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| delete(c, "ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn round_trip_via_the_project_struct() {
    // Smoke test: serialise + deserialise a Project and confirm the wire form
    // matches the TS interface.
    let p = Project {
        id: "p1".into(),
        name: "x".into(),
        description: Some("说明".into()),
        color: "#fff".into(),
        created_at: 1,
        updated_at: 2,
    };
    let s = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&s).unwrap();
    assert_eq!(back.name, "x");
    assert_eq!(back.description.as_deref(), Some("说明"));
    assert_eq!(back.color, "#fff");
    assert_eq!(back.created_at, 1);
}

#[test]
fn create_with_defaults_seeds_a_project_and_one_environment() {
    let pool = fresh_pool();
    let created = db::with_tx(&pool, |c| {
        create_with_defaults(c, &sample_project("p_new", "New Project"))
    })
    .unwrap();

    assert_eq!(created.id, "p_new");

    let envs = db::with_conn(&pool, |c| db::environments::list_by_project(c, "p_new")).unwrap();
    assert_eq!(envs.len(), 1, "exactly the default environment");
    assert_eq!(envs[0].name, DEFAULT_ENVIRONMENT_NAME);
    assert!(
        envs[0].created_at > 0,
        "it has to carry a creation time — the default environment is the earliest one"
    );
    assert!(
        envs[0].module_base_urls.is_empty(),
        "and it has no values to resolve against yet"
    );

    // And the row is really there, not just in the echo.
    let got = db::with_conn(&pool, |c| get(c, "p_new")).unwrap();
    assert_eq!(got.id, "p_new");
}

#[test]
fn create_with_defaults_seeds_one_module_at_the_project_root() {
    // 模块既是 base URL 表格的行轴，又是模块工作区的主题；没有它，一个全新项目会同时摆出两个
    // 死路——树的「该项目还没有模块」与表格的「没有模块可归属」——而这是一张用户反正会用到的形状。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create_with_defaults(c, &sample_project("p_new", "New Project"))
    })
    .unwrap();

    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, "p_new")).unwrap();
    assert_eq!(nodes.len(), 1, "exactly the default module");
    let module = &nodes[0];
    assert_eq!(module.kind, ResourceNodeType::Module);
    assert_eq!(module.name, DEFAULT_MODULE_NAME);
    assert_eq!(
        module.root_id, "p_new",
        "root_id is what says which project owns the row — the nodes table \
         has no project row to inherit it from"
    );
    assert_eq!(
        module.parent_id, None,
        "a top-level module's parent is the project, which stores as NULL"
    );
}

#[test]
fn create_with_defaults_seeds_the_environment_and_the_module_in_one_transaction() {
    // 种子只有在项目存在时才有意义：一个写了一半的项目会渲染成「模块已经可见，却没有环境
    // 可供解析 {{var}}」，或反过来。
    let pool = fresh_pool();
    let outcome = db::with_tx(&pool, |c| {
        create_with_defaults(c, &sample_project("p_conflict", "New Project"))
    });
    assert!(outcome.is_ok());

    let envs = db::with_conn(&pool, |c| {
        db::environments::list_by_project(c, "p_conflict")
    })
    .unwrap();
    let modules = db::with_conn(&pool, |c| db::nodes::list_by_root(c, "p_conflict")).unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].root_id, "p_conflict");
}

#[test]
fn update_ignores_a_payload_that_carries_a_retired_key() {
    // `ProjectPatch` 不收陌生键，故旧客户端载荷里的 `baseUrl` 不会变成一条
    // `no such column` 的数据库故障——它只是被丢掉。
    let pool = fresh_pool();
    let patched = db::with_tx(&pool, |c| {
        update(
            c,
            "p_demo",
            serde_json::from_str(r#"{"baseUrl": "http://ignored"}"#).unwrap(),
        )
    })
    .unwrap();
    assert_eq!(patched.id, "p_demo");
    assert_eq!(patched.name, "Demo API");
}
