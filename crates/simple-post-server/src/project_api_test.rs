//! 项目与资源树节点的创建编排 —— 两侧 transport 共用的那一份行为。
//!
//! 这里断言的是**规则**（id 归谁铸、父子关系的推导、两半一起落），
//! 不是某一条线的形状 —— Tauri 命令与 REST 路由的线上形状各自不同，它们只是把各自的
//! 形状翻译成本模块的入参。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{ResourceNode, ResourceNodeType};

use crate::dto::{CreateEndpointDetailOpts, CreateProjectOpts};

use super::{create_node, create_project, resolve_root_id, NodeCreateInput};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 每次测试一份自己的库。隔离靠原子计数器加 pid，因为 cargo 并行跑测试，好几个测试会
/// 共用同一条临时路径。
fn temp_pool(tag: &str) -> DbPool {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-project-api-{tag}-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(Path::new(&dir)).expect("db init")
}

/// 一个项目：返回它的 id。
fn seed_project(pool: &DbPool, id: &str) -> String {
    let project = create_project(
        pool,
        Some(id.to_string()),
        "示例".to_string(),
        CreateProjectOpts::default(),
    )
    .expect("建项目");
    project.id
}

/// 项目里那个默认模块的 id。
fn default_module_id(pool: &DbPool, project_id: &str) -> String {
    let modules = db::with_conn(pool, |c| {
        db::nodes::list_by_root_and_type(c, project_id, ResourceNodeType::Module)
    })
    .expect("读模块");
    modules
        .into_iter()
        .find(|m| m.parent_id.is_none())
        .expect("项目有一个默认模块")
        .id
}

/// 一个节点行，除 id / 种类 / 名字外都是常见值。
fn node(id: &str, kind: ResourceNodeType, root_id: &str, parent_id: Option<&str>) -> ResourceNode {
    ResourceNode {
        id: id.to_string(),
        root_id: root_id.to_string(),
        parent_id: parent_id.map(str::to_string),
        kind,
        name: id.to_string(),
        description: None,
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    }
}

// ----- 项目 ---------------------------------------------------------------

#[test]
fn a_project_without_a_caller_given_id_gets_one() {
    let pool = temp_pool("mint");

    let project = create_project(
        &pool,
        None,
        "示例".to_string(),
        CreateProjectOpts::default(),
    )
    .expect("建项目");

    assert!(!project.id.is_empty(), "没有 id 就该铸一个");
}

#[test]
fn a_project_keeps_the_id_the_caller_gave() {
    let pool = temp_pool("adopt");

    let project = create_project(
        &pool,
        Some("p_given".to_string()),
        "示例".to_string(),
        CreateProjectOpts::default(),
    )
    .expect("建项目");

    assert_eq!(project.id, "p_given");
}

#[test]
fn a_project_with_an_empty_id_gets_a_fresh_one() {
    let pool = temp_pool("empty-id");

    let project = create_project(
        &pool,
        Some(String::new()),
        "示例".to_string(),
        CreateProjectOpts::default(),
    )
    .expect("建项目");

    assert!(!project.id.is_empty(), "空 id 与缺席一样，都该铸一个");
}

#[test]
fn a_project_keeps_a_blank_id_the_caller_gave() {
    let pool = temp_pool("blank-id");

    let project = create_project(
        &pool,
        Some("   ".to_string()),
        "示例".to_string(),
        CreateProjectOpts::default(),
    )
    .expect("建项目");

    // 项目 id 是调用方给的标识，它给什么就是什么。纯空白也算「给了」，故照用。
    assert_eq!(project.id, "   ", "纯空白也是调用方给的 id，要照用");
}

#[test]
fn a_new_project_gets_a_default_environment_and_a_default_module() {
    let pool = temp_pool("defaults");
    let project_id = seed_project(&pool, "p_defaults");

    let envs = db::with_conn(&pool, |c| db::environments::list_by_project(c, &project_id))
        .expect("读环境");
    assert_eq!(envs.len(), 1, "项目一存在就有一个环境可解析 base URL");
    assert!(envs[0].is_active, "默认环境是激活态");

    let modules = db::with_conn(&pool, |c| {
        db::nodes::list_by_root_and_type(c, &project_id, ResourceNodeType::Module)
    })
    .expect("读模块");
    assert_eq!(modules.len(), 1, "项目一存在就有一个模块可归属");
}

// ----- 节点 ---------------------------------------------------------------

#[test]
fn an_endpoint_is_written_together_with_its_request_config() {
    let pool = temp_pool("endpoint");
    let project_id = seed_project(&pool, "p_endpoint");
    let module_id = default_module_id(&pool, &project_id);

    let created = create_node(
        &pool,
        NodeCreateInput {
            node: node(
                "e1",
                ResourceNodeType::Endpoint,
                &project_id,
                Some(&module_id),
            ),
            detail: Some(CreateEndpointDetailOpts {
                method: Some(simple_post_core::domain::HttpMethod::Post),
                url: Some("/orders".to_string()),
                ..Default::default()
            }),
        },
    )
    .expect("建 endpoint");

    assert_eq!(created.id, "e1");
    let detail = db::with_conn(&pool, |c| db::endpoint_details::get(c, "e1")).expect("读请求配置");
    assert_eq!(detail.node_id, "e1");
    assert_eq!(detail.url, "/orders");
}

#[test]
fn an_endpoint_without_a_request_config_is_refused_and_nothing_is_written() {
    let pool = temp_pool("endpoint-bare");
    let project_id = seed_project(&pool, "p_endpoint_bare");
    let module_id = default_module_id(&pool, &project_id);

    let err = create_node(
        &pool,
        NodeCreateInput {
            node: node(
                "e_bare",
                ResourceNodeType::Endpoint,
                &project_id,
                Some(&module_id),
            ),
            detail: None,
        },
    )
    .expect_err("没有请求配置的 endpoint 不该建得出来");

    assert!(
        matches!(err, simple_post_core::error::AppError::Validation(_)),
        "该报校验错误，实际报的是：{err:?}"
    );
    let found = db::with_conn(&pool, |c| db::nodes::get(c, "e_bare")).is_ok();
    assert!(!found, "被拒的那一次不能留下半个节点");
}

#[test]
fn a_collection_with_a_request_config_is_refused() {
    let pool = temp_pool("collection-detail");
    let project_id = seed_project(&pool, "p_collection");
    let module_id = default_module_id(&pool, &project_id);

    let err = create_node(
        &pool,
        NodeCreateInput {
            node: node(
                "c1",
                ResourceNodeType::Collection,
                &project_id,
                Some(&module_id),
            ),
            detail: Some(CreateEndpointDetailOpts::default()),
        },
    )
    .expect_err("集合不该带请求配置");

    assert!(
        matches!(err, simple_post_core::error::AppError::Validation(_)),
        "该报校验错误，实际报的是：{err:?}"
    );
}

#[test]
fn a_child_node_takes_the_project_of_its_parent() {
    let pool = temp_pool("derive-root");
    let project_id = seed_project(&pool, "p_derive");
    let module_id = default_module_id(&pool, &project_id);

    let created = create_node(
        &pool,
        NodeCreateInput {
            // root_id 留空：子节点的归属由父节点蕴含。
            node: node("e_derive", ResourceNodeType::Endpoint, "", Some(&module_id)),
            detail: Some(CreateEndpointDetailOpts::default()),
        },
    )
    .expect("建 endpoint");

    assert_eq!(
        created.root_id, project_id,
        "父节点在哪个项目，子节点就在哪个项目"
    );
}

#[test]
fn a_node_keeps_the_root_id_the_caller_gave() {
    let pool = temp_pool("explicit-root");
    let project_id = seed_project(&pool, "p_explicit");

    // 顶层模块：没有父节点可推导，故调用方给的 root_id 是唯一的依据。若这一层改成
    // 一律从父节点推导，root_id 会是空串，落库时会被 `validate_placement` 拒掉
    // （「顶层节点要说出它的项目」）。
    let created = create_node(
        &pool,
        NodeCreateInput {
            node: node("m_explicit", ResourceNodeType::Module, &project_id, None),
            detail: None,
        },
    )
    .expect("建顶层模块");

    assert_eq!(created.root_id, project_id);
}

#[test]
fn resolving_the_root_id_answers_the_same_way_whichever_way_it_is_asked() {
    let pool = temp_pool("resolve");
    let project_id = seed_project(&pool, "p_resolve");
    let module_id = default_module_id(&pool, &project_id);
    // 另一个项目：它的模块当父节点，而调用方点名的是上面那个项目。两者不同时，
    // 「照用调用方给的」与「一律从父节点查」才是两个不同的答案——父节点与调用方
    // 点名的项目相同时，两条路算出来一样，断言就没有分辨力。
    let other_project_id = seed_project(&pool, "p_resolve_other");
    let other_module_id = default_module_id(&pool, &other_project_id);

    assert_eq!(
        resolve_root_id(&pool, Some(&project_id), Some(&other_module_id)).expect("读项目"),
        project_id,
        "调用方点名了项目就用它，不去查父节点"
    );
    assert_eq!(
        resolve_root_id(&pool, None, Some(&module_id)).expect("读父节点"),
        project_id,
        "没点名项目就从父节点那里查出来"
    );
    assert_eq!(
        resolve_root_id(&pool, None, None).expect("什么都没有"),
        String::new(),
        "两样都没点名就没有答案，不是猜一个"
    );
}
