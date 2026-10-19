//! 落库：结构与 IR 一致、非法放置被拒、重复落库不产生副本。
//!
//! 测试池自己建：core 的 `test_support` 是 `pub(crate)`，跨 crate 不可达，
//! 故这里照 `db::init` 的形状走一遍，得到的是一个没有种子行的真实空库。

use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    uid, AuthConfig, BodyMode, HttpMethod, KeyValue, Project, RequestBody, ResourceNodeType,
};
use simple_post_core::error::AppError;

use super::commit;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule};

static COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// module → collection → collection → endpoint 的多层嵌套，外加一个直接挂在
/// 模块下的 endpoint 与一个挂在中间 collection 下的 endpoint。
fn nested_exchange() -> Exchange {
    Exchange {
        project_name: "p".into(),
        modules: vec![IrModule {
            name: "用户".into(),
            endpoints: vec![IrEndpoint::new("直挂的接口", HttpMethod::Get, "/ping")],
            collections: vec![IrCollection {
                name: "账号".into(),
                endpoints: vec![IrEndpoint::new("登录", HttpMethod::Post, "/login")],
                collections: vec![IrCollection {
                    name: "令牌".into(),
                    endpoints: vec![IrEndpoint::new("刷新", HttpMethod::Post, "/token/refresh")],
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

    let report = commit(&pool, &project_id, &ex).expect("commit");
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

    let report = commit(&pool, &project_id, &ex).expect("commit");
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

    let first = commit(&pool, &project_id, &ex).expect("first");
    assert_eq!(first.created_endpoint_ids.len(), 3);

    let second = commit(&pool, &project_id, &ex).expect("second");
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
    commit(&pool, &project_id, &nested_exchange()).expect("first");

    let mut renamed = nested_exchange();
    renamed.modules[0].endpoints[0].name = "改了名字".into();
    renamed.modules[0].endpoints[0].description = Some("补了描述".into());
    let second = commit(&pool, &project_id, &renamed).expect("second");

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
    let dup = ex.modules[0].endpoints[0].clone();
    ex.modules[0].endpoints.push(dup);

    let report = commit(&pool, &project_id, &ex).expect("commit");
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
    let err = commit(&pool, "proj_does_not_exist", &nested_exchange()).expect_err("must fail");
    assert!(matches!(err, AppError::NotFound(_)));

    // 目标项目自己一个节点也没多出来。
    let nodes = db::with_conn(&pool, |c| db::nodes::list_by_root(c, &project_id)).expect("list");
    assert_eq!(nodes.len(), 1, "only the seeded default module");
}
