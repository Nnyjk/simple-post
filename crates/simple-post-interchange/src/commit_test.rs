//! 落库：结构与 IR 一致、非法放置被拒、重复落库不产生副本、按勾选裁剪。
//!
//! 测试池自己建：core 的 `test_support` 是 `pub(crate)`，跨 crate 不可达，
//! 故这里照 `db::init` 的形状走一遍，得到的是一个没有种子行的真实空库。

use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    uid, AuthConfig, BodyMode, HttpMethod, KeyValue, Project, RequestBody, ResourceNodeType,
};
use simple_post_core::error::AppError;

use super::{commit, CommitReport, CommitTarget};
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 「勾满 + 落项目顶层」。绝大多数落库测试要的是这个形状，故给一个短名字。
fn commit_all(pool: &DbPool, project_id: &str, ex: &Exchange) -> Result<CommitReport, AppError> {
    commit(
        pool,
        project_id,
        CommitTarget::ProjectRoot,
        ex,
        &ex.endpoint_keys(),
    )
}

/// 一个空的临时库 + 一个带默认环境与模块的项目。
fn fresh_project() -> (DbPool, String) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-interchange-commit-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    let project = db::with_tx(&pool, |conn| {
        db::projects::create_with_defaults(
            conn,
            &Project {
                id: uid(),
                name: "测试项目".into(),
                description: None,
                base_url: None,
                base_url_definitions: Vec::new(),
                default_base_url_definition_id: None,
                color: "#3b82f6".into(),
                created_at: 0,
                updated_at: 0,
            },
        )
    })
    .expect("create project");
    (pool, project.id)
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        id: String::new(),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }
}

/// `create_with_defaults` 种下的那个模块。导入的落点用它。
fn seeded_module(pool: &DbPool, project_id: &str) -> String {
    db::with_conn(pool, |c| {
        db::nodes::list_by_root(c, project_id)?
            .into_iter()
            .find(|n| n.kind == ResourceNodeType::Module)
            .map(|n| n.id)
            .ok_or_else(|| AppError::NotFound("no seeded module".into()))
    })
    .expect("seeded module")
}

/// module → collection → collection → endpoint 的多层嵌套，外加一个直接挂在
/// 模块下的 endpoint 与一个挂在中间 collection 下的 endpoint。
///
/// `key` 照源格式的寻址习惯给（`/item/0` 那种形状），勾选测试靠它选中具体某一行。
fn nested_exchange() -> Exchange {
    Exchange {
        project_name: "p".into(),
        modules: vec![IrModule {
            key: "/item/0".into(),
            name: "用户".into(),
            endpoints: vec![IrEndpoint {
                key: "/item/0/request/0".into(),
                ..IrEndpoint::new("直挂的接口", HttpMethod::Get, "/ping")
            }],
            collections: vec![IrCollection {
                key: "/item/0/item/0".into(),
                name: "账号".into(),
                endpoints: vec![IrEndpoint {
                    key: "/item/0/item/0/request/0".into(),
                    ..IrEndpoint::new("登录", HttpMethod::Post, "/login")
                }],
                collections: vec![IrCollection {
                    key: "/item/0/item/0/item/0".into(),
                    name: "令牌".into(),
                    endpoints: vec![IrEndpoint {
                        key: "/item/0/item/0/item/0/request/0".into(),
                        ..IrEndpoint::new("刷新", HttpMethod::Post, "/token/refresh")
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn a_nested_ir_lands_in_one_transaction() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();

    let report = commit_all(&pool, &project_id, &ex).expect("commit");
    assert_eq!(report.created_endpoint_ids.len(), 3);
    assert!(report.skipped_duplicates.is_empty());

    // 写入顺序跟 IR 的声明顺序一致：先本层 endpoint，再各 collection 依次深入。
    let paths: Vec<String> = db::with_conn(&pool, |c| {
        report
            .created_endpoint_ids
            .iter()
            .map(|id| db::endpoint_details::get(c, id).map(|d| d.url))
            .collect::<Result<_, _>>()
    })
    .expect("details");
    assert_eq!(paths, vec!["/ping", "/login", "/token/refresh"]);

    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    // 项目自带一个「默认模块」，加上本次导入的 1 模块 + 2 collection + 3 endpoint。
    assert_eq!(nodes.len(), 7);

    let names_of = |k: ResourceNodeType| -> Vec<String> {
        let mut v: Vec<String> = nodes
            .iter()
            .filter(|n| n.kind == k)
            .map(|n| n.name.clone())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names_of(ResourceNodeType::Module), vec!["用户", "默认模块"]);
    assert_eq!(names_of(ResourceNodeType::Collection), vec!["令牌", "账号"]);
    // 按名字找导入进来的那个模块——项目自带的「默认模块」与它无关。
    let find = |name: &str| nodes.iter().find(|n| n.name == name).expect(name).clone();
    let module = find("用户");
    assert_eq!(
        find("账号").parent_id.as_deref(),
        Some(module.id.as_str()),
        "collection 的父是模块"
    );
    assert_eq!(
        find("令牌").parent_id.as_deref(),
        Some(find("账号").id.as_str()),
        "嵌套 collection 的父是外层 collection"
    );
    assert_eq!(
        find("登录").parent_id.as_deref(),
        Some(find("账号").id.as_str())
    );
    assert_eq!(
        find("直挂的接口").parent_id.as_deref(),
        Some(module.id.as_str()),
        "endpoint 可以直接挂在模块下"
    );
    assert_eq!(module.parent_id, None, "模块的父是项目本身，存为 NULL");
}

#[test]
fn endpoint_details_land_with_their_nodes() {
    let (pool, project_id) = fresh_project();
    let mut ex = nested_exchange();
    ex.modules[0].endpoints[0].headers = vec![kv("X-A", "1")];
    ex.modules[0].endpoints[0].body = RequestBody {
        mode: BodyMode::Json,
        content: "{\"a\":1}".into(),
        schema: None,
    };
    ex.modules[0].endpoints[0].auth = AuthConfig {
        kind: simple_post_core::domain::AuthType::Bearer,
        bearer: Some("t".into()),
        ..Default::default()
    };

    let report = commit_all(&pool, &project_id, &ex).expect("commit");
    let detail = db::with_conn(&pool, |c| {
        db::endpoint_details::get(c, &report.created_endpoint_ids[0])
    })
    .expect("detail");

    assert_eq!(detail.url, "/ping");
    assert_eq!(detail.headers.len(), 1);
    assert_eq!(detail.body.mode, BodyMode::Json);
    assert_eq!(detail.auth.kind, simple_post_core::domain::AuthType::Bearer);
    // 外来格式给不出服务与环境，写 None 是真实答案（「用项目的 ★」）。
    assert_eq!(detail.base_url_definition_id, None);
}

#[test]
fn committing_the_same_ir_twice_creates_no_duplicate() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();

    let first = commit_all(&pool, &project_id, &ex).expect("first");
    assert_eq!(first.created_endpoint_ids.len(), 3);

    let second = commit_all(&pool, &project_id, &ex).expect("second");
    assert!(
        second.created_endpoint_ids.is_empty(),
        "second pass created {:?}",
        second.created_endpoint_ids
    );
    assert_eq!(second.skipped_duplicates.len(), 3);
    // 跳过要有出处：一条 Info 说清按指纹跳过，且没有更严重的诊断。
    assert_eq!(second.notable_diagnostics().len(), 0);
    assert!(second
        .diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Info));

    let count = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &project_id)
    })
    .expect("list")
    .len();
    assert_eq!(count, 3);
}

#[test]
fn a_renamed_endpoint_is_still_recognised_as_the_same_one() {
    let (pool, project_id) = fresh_project();
    commit_all(&pool, &project_id, &nested_exchange()).expect("first");

    let mut renamed = nested_exchange();
    renamed.modules[0].endpoints[0].name = "改了名字".into();
    renamed.modules[0].endpoints[0].description = Some("补了描述".into());
    let second = commit_all(&pool, &project_id, &renamed).expect("second");

    // 三个接口一个都没变（名字与描述不进指纹），故全部被认出来。
    assert!(
        second.created_endpoint_ids.is_empty(),
        "改名不该造出副本, created = {:?}",
        second.created_endpoint_ids
    );
    assert_eq!(second.skipped_duplicates.len(), 3);
    let count = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &project_id)
    })
    .expect("list")
    .len();
    assert_eq!(count, 3);
}

#[test]
fn a_duplicate_inside_one_ir_lands_once() {
    let (pool, project_id) = fresh_project();
    let mut ex = nested_exchange();
    // 同一个 IR 节点被复制了一份——key 也一样。真实外来文件不会这样（key 是源
    // 格式里的位置指针），故这条测的是「解析器给出重复 key」时失败形态是什么：
    // 落一份，不是落两份。
    let dup = ex.modules[0].endpoints[0].clone();
    ex.modules[0].endpoints.push(dup);

    let report = commit_all(&pool, &project_id, &ex).expect("commit");
    assert_eq!(report.created_endpoint_ids.len(), 3, "重复项第二次被跳过");
    assert_eq!(report.skipped_duplicates, vec!["直挂的接口"]);
}

#[test]
fn an_illegal_placement_is_rejected_and_names_the_conflict() {
    let (pool, project_id) = fresh_project();

    // IR 的类型系统让「module 下挂 module」写不出来，所以这里直接打落库那条路，
    // 证明拒绝来自 `validate_placement` 而不是编译器。
    let err = db::with_tx(&pool, |conn| {
        let outer = simple_post_core::domain::ResourceNode {
            id: uid(),
            root_id: project_id.clone(),
            parent_id: None,
            kind: ResourceNodeType::Module,
            name: "外层".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let outer = db::nodes::create(conn, &outer, None)?;
        let inner = simple_post_core::domain::ResourceNode {
            id: uid(),
            root_id: project_id.clone(),
            parent_id: Some(outer.id),
            kind: ResourceNodeType::Module,
            name: "内层".into(),
            description: None,
            sort_order: 1,
            created_at: 0,
            updated_at: 0,
        };
        db::nodes::create(conn, &inner, None)?;
        Ok::<(), AppError>(())
    })
    .expect_err("module under module must be refused");

    let msg = err.to_string();
    assert!(
        msg.contains("a module cannot sit under a module"),
        "错误必须点名冲突的那一对类型, got: {msg}"
    );

    // 事务回滚：内层那一行没有留下。
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    assert!(
        nodes.iter().all(|n| n.name != "内层"),
        "失败的事务不该留下任何行"
    );
}

#[test]
fn the_ir_placement_matrix_matches_the_domain_one() {
    // commit 不自己判放置，走的是 `db::nodes::create` 的校验。故断言 IR 侧的
    // `may_contain` 与 domain 的 `may_under` 是同一个矩阵，而不是第二份真相。
    use simple_post_core::domain::ResourceNodeType as T;
    for parent in [T::Module, T::Collection, T::Endpoint] {
        for child in [T::Module, T::Collection, T::Endpoint] {
            assert_eq!(
                crate::ir::may_contain(parent, child).is_ok(),
                child.may_under(parent),
                "{parent:?} → {child:?}"
            );
        }
    }
}

#[test]
fn a_failed_commit_leaves_no_half_tree() {
    let (pool, project_id) = fresh_project();

    // 指向一个不存在的项目。
    let err = commit_all(&pool, "proj_does_not_exist", &nested_exchange()).expect_err("must fail");
    assert!(matches!(err, AppError::NotFound(_)));

    // 目标项目自己一个节点也没多出来。
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    assert_eq!(nodes.len(), 1, "only the seeded default module");
}

// -------------------------------------------------------------------------
// 落进已有模块 + 按勾选裁剪。
// -------------------------------------------------------------------------

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
    )
    .expect_err("跨项目落点必须被拒");
    assert!(matches!(err, AppError::NotFound(_)), "got: {err}");
}
