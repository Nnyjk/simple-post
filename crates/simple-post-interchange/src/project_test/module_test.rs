//! module_to_ir —— 只落一个模块的子树，并拒绝两类非法落点。

use simple_post_core::db;
use simple_post_core::domain::{uid, ResourceNode, ResourceNodeType};
use simple_post_core::error::AppError;

use super::support::{add_endpoint, seed_project, temp_pool};

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
