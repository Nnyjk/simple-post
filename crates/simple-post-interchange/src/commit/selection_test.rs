//! 落进已有模块 + 按勾选裁剪：顶层降级成集合、祖先留住、空分组不建、落点校验。

use simple_post_core::db;
use simple_post_core::domain::{uid, ResourceNodeType};
use simple_post_core::error::AppError;

use super::support::{commit_all, fresh_project, nested_exchange, seeded_module};
use super::{commit, CommitTarget};

#[test]
fn a_module_target_demotes_top_level_groups_to_collections() {
    let (pool, project_id) = fresh_project();
    let anchor = seeded_module(&pool, &project_id);
    let ex = nested_exchange();

    let report = commit(
        &pool,
        &project_id,
        CommitTarget::Module { module_id: &anchor },
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect("commit");

    // 顶层那个 IR 模块落成了集合——模块只能待在项目顶层，这是树契约强制的。
    assert_eq!(report.created_collection_ids.len(), 3, "1 顶层 + 2 嵌套");
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    let by_name = |n: &str| nodes.iter().find(|x| x.name == n).expect(n).clone();

    let users = by_name("用户");
    assert_eq!(users.kind, ResourceNodeType::Collection);
    assert_eq!(users.parent_id.as_deref(), Some(anchor.as_str()));
    assert_eq!(
        by_name("账号").parent_id.as_deref(),
        Some(users.id.as_str())
    );
    // 项目里不该多出第二个模块。
    let modules: Vec<_> = nodes
        .iter()
        .filter(|n| n.kind == ResourceNodeType::Module)
        .collect();
    assert_eq!(modules.len(), 1, "only the seeded module remains");
}

#[test]
fn selecting_only_a_leaf_keeps_every_ancestor() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();
    // 只勾最深处那一个接口。
    let selected: std::collections::BTreeSet<String> =
        ["/item/0/item/0/item/0/request/0".to_string()]
            .into_iter()
            .collect();

    let report = commit(
        &pool,
        &project_id,
        CommitTarget::ProjectRoot,
        &ex,
        &selected,
        true,
    )
    .expect("commit");

    assert_eq!(report.created_endpoint_ids.len(), 1);
    let detail = db::with_conn(&pool, |c| {
        db::endpoint_details::get(c, &report.created_endpoint_ids[0])
    })
    .expect("detail");
    assert_eq!(detail.url, "/token/refresh");

    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    let by_name = |n: &str| nodes.iter().find(|x| x.name == n).expect(n).clone();
    // 路径上的两个集合都被建出来了，否则这个接口会挂在一个不存在的父下。
    assert_eq!(by_name("用户").kind, ResourceNodeType::Module);
    assert_eq!(
        by_name("账号").parent_id.as_deref(),
        Some(by_name("用户").id.as_str())
    );
    assert_eq!(
        by_name("令牌").parent_id.as_deref(),
        Some(by_name("账号").id.as_str())
    );
    // 没勾的同级接口一个都没落。
    assert!(nodes
        .iter()
        .all(|n| n.name != "登录" && n.name != "直挂的接口"));
}

#[test]
fn deselecting_a_whole_group_creates_no_empty_collection() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();
    // 三个接口全不勾。
    let selected = std::collections::BTreeSet::new();

    let report = commit(
        &pool,
        &project_id,
        CommitTarget::ProjectRoot,
        &ex,
        &selected,
        true,
    )
    .expect("取消勾满不该算失败");

    assert!(report.created_endpoint_ids.is_empty());
    assert!(report.created_collection_ids.is_empty());
    assert!(
        report.diagnostics.is_empty(),
        "用户主动取消勾选不是问题，不该发诊断: {:?}",
        report.diagnostics
    );
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    assert_eq!(nodes.len(), 1, "只该剩下那个种子模块");
}

#[test]
fn a_selected_duplicate_is_still_skipped_and_leaves_no_empty_groups() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();
    commit_all(&pool, &project_id, &ex).expect("first");
    let after_first = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id))
        .expect("list")
        .len();

    // 第二次勾满全部——但它们全都已经在了。
    let second = commit_all(&pool, &project_id, &ex).expect("second");
    assert!(second.created_endpoint_ids.is_empty());
    assert_eq!(second.skipped_duplicates.len(), 3);
    // 勾满了不等于写进去了。三行都没新增，容器也就一个都不该留下——否则树里会
    // 多出一排点开什么都没有的集合。
    assert!(
        second.created_collection_ids.is_empty(),
        "空分组不该被建出来: {:?}",
        second.created_collection_ids
    );
    let after_second = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id))
        .expect("list")
        .len();
    assert_eq!(after_second, after_first, "树不该有任何变化");
}

#[test]
fn a_deselected_endpoint_is_never_even_considered() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();
    commit_all(&pool, &project_id, &ex).expect("first");

    // 第二次只勾「直挂的接口」——它已经在了。
    let selected: std::collections::BTreeSet<String> =
        ["/item/0/request/0".to_string()].into_iter().collect();
    let report = commit(
        &pool,
        &project_id,
        CommitTarget::ProjectRoot,
        &ex,
        &selected,
        true,
    )
    .expect("commit");

    assert!(report.created_endpoint_ids.is_empty());
    // 勾中的那个走指纹：被认出、不新增，且有跳过说明。
    assert_eq!(report.skipped_duplicates, vec!["直挂的接口"]);
    // 没勾的两个既不写、也不进重复账——用户取消勾选不是「跳过理由」，
    // 多说一条只会把真正该看的诊断淹掉。
    let skipped: Vec<&str> = report
        .diagnostics
        .iter()
        .filter_map(|d| d.dropped.as_deref())
        .collect();
    assert_eq!(skipped, vec!["GET /ping"], "只有勾中的那个有跳过说明");
}

#[test]
fn an_anchor_that_is_not_a_module_is_rejected() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();
    let anchor = seeded_module(&pool, &project_id);
    let collection_id = db::with_tx(&pool, |conn| {
        db::nodes::create(
            conn,
            &simple_post_core::domain::ResourceNode {
                id: uid(),
                root_id: project_id.clone(),
                parent_id: Some(anchor.clone()),
                kind: ResourceNodeType::Collection,
                name: "某个集合".into(),
                description: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            },
            None,
        )
    })
    .expect("create collection")
    .id;

    let err = commit(
        &pool,
        &project_id,
        CommitTarget::Module {
            module_id: &collection_id,
        },
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect_err("集合不是合法落点");
    assert!(
        matches!(&err, AppError::Validation(m) if m.contains("导入落点必须是模块")),
        "got: {err}"
    );
}

#[test]
fn an_anchor_from_another_project_is_rejected() {
    let (pool, project_id) = fresh_project();
    let (other_pool, other_project) = fresh_project();
    let ex = nested_exchange();
    // 从另一个连接池里借一个 id：跨项目串进来会让 URL 与权限门禁都失去意义。
    let foreign = seeded_module(&other_pool, &other_project);

    let err = commit(
        &pool,
        &project_id,
        CommitTarget::Module {
            module_id: &foreign,
        },
        &ex,
        &ex.endpoint_keys(),
        true,
    )
    .expect_err("跨项目落点必须被拒");
    assert!(matches!(err, AppError::NotFound(_)), "got: {err}");
}
