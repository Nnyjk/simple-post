//! 建环境的两条 transport 共用的那一份行为。
//!
//! 这里断言的是**规则**，不是任何一条线的形状：id 归谁铸、创建时刻由谁补、默认环境是谁。
//! REST 侧与桌面侧只是把各自的线上形状翻成本模块的入参，故两侧的行为由这一份用例钉住。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::Project;

use crate::dto::CreateEnvironmentOpts;

use super::create_environment;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 每次测试一份自己的库（同 `project_api_test.rs`：cargo 并行跑，路径必须唯一）。
fn temp_pool(tag: &str) -> DbPool {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-environment-api-{tag}-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(Path::new(&dir)).expect("db init")
}

/// 一个**没有环境**的项目。
///
/// 刻意不走 `create_project`（或 `projects::create_with_defaults`）：那两条会种下一个
/// `created_at = now_ms()` 的默认环境，于是「最早创建的那个」永远不是本用例建的那一个，
/// 断言就空转了。这里只插项目行本身，让环境表是空的。
fn seed_project_without_envs(pool: &DbPool, id: &str) {
    let mut project = Project::empty();
    project.id = id.to_string();
    project.name = "示例".to_string();
    db::with_tx(pool, |c| db::projects::create(c, &project)).expect("建项目");
}

fn read_row(pool: &DbPool, id: &str) -> simple_post_core::domain::Environment {
    db::with_conn(pool, |c| db::environments::get(c, id)).expect("读环境")
}

/// 同一项目连续建两个：创建时刻递增（同一毫秒内相等也算），**第一个**是默认环境。
///
/// 这一条钉的正是「web 建的环境抢默认位」那种事故：任何一条 transport 把 `created_at`
/// 写死成 0，第一个断言或默认环境断言就会红。
#[test]
fn two_environments_in_a_row_leave_the_first_one_as_the_default() {
    let pool = temp_pool("order");
    seed_project_without_envs(&pool, "p1");

    let first = create_environment(
        &pool,
        None,
        "p1".to_string(),
        "dev".to_string(),
        CreateEnvironmentOpts::default(),
    )
    .expect("建第一个环境");
    let second = create_environment(
        &pool,
        None,
        "p1".to_string(),
        "prod".to_string(),
        CreateEnvironmentOpts::default(),
    )
    .expect("建第二个环境");

    // 创建时刻由 `db::environments::create` 补，不吃调用方手里的 0。
    assert!(
        first.created_at > 0,
        "创建时刻必须是服务端补的当前时刻，不能是 0"
    );
    assert!(
        second.created_at >= first.created_at,
        "后建的环境不能排在先建的之前：{} < {}",
        second.created_at,
        first.created_at
    );
    // 回传的就是库里那一行 —— 界面不必再刷新一次才能算出默认环境。
    assert_eq!(first.created_at, read_row(&pool, &first.id).created_at);
    assert_eq!(second.created_at, read_row(&pool, &second.id).created_at);

    let default = db::with_conn(&pool, |c| db::environments::get_default(c, "p1"))
        .expect("读默认环境")
        .expect("项目里有环境");
    assert_eq!(
        default.id, first.id,
        "最早创建的那个才是默认环境，新环境排在它后面"
    );
    assert_ne!(first.id, second.id, "两个环境不能共用一个 id");
}

/// 结构那半边由共享层定：空模块表、空停用集合、id 服务端铸。
#[test]
fn a_new_environment_arrives_with_the_shared_skeleton() {
    let pool = temp_pool("skeleton");
    seed_project_without_envs(&pool, "p1");

    let env = create_environment(
        &pool,
        None,
        "p1".to_string(),
        db::projects::DEFAULT_ENVIRONMENT_NAME.to_string(),
        CreateEnvironmentOpts::default(),
    )
    .expect("建环境");

    assert_eq!(env.project_id, "p1");
    assert_eq!(env.name, db::projects::DEFAULT_ENVIRONMENT_NAME);
    assert!(!env.id.is_empty(), "id 由服务端铸，不能是空串");
    assert!(
        env.module_base_urls.is_empty(),
        "URL 只在（环境 × 模块）那一格"
    );
    assert!(env.variables.is_empty());
    assert!(env.disabled_variables.is_empty());
}

/// 调用方给的 id 照用（REST 的 `ensure_server_id` 与打平 v1 形状都靠它），
/// 给的变量落进那一行。
#[test]
fn a_caller_supplied_id_is_adopted_and_variables_are_stored() {
    let pool = temp_pool("adopt");
    seed_project_without_envs(&pool, "p1");

    let mut vars = std::collections::HashMap::new();
    vars.insert("host".to_string(), "api.example.test".to_string());
    let env = create_environment(
        &pool,
        Some("env_fixed".to_string()),
        "p1".to_string(),
        "staging".to_string(),
        CreateEnvironmentOpts {
            variables: Some(vars),
        },
    )
    .expect("建环境");

    assert_eq!(env.id, "env_fixed", "调用方点名了 id 就照用");
    assert_eq!(
        env.variables.get("host").map(String::as_str),
        Some("api.example.test")
    );
    assert_eq!(read_row(&pool, "env_fixed").created_at, env.created_at);
}

/// 空 id 与缺席一样：都铸一个，而不是写一行空 id 进库。
#[test]
fn an_empty_id_is_minted_instead_of_written() {
    let pool = temp_pool("empty-id");
    seed_project_without_envs(&pool, "p1");

    let env = create_environment(
        &pool,
        Some(String::new()),
        "p1".to_string(),
        "dev".to_string(),
        CreateEnvironmentOpts::default(),
    )
    .expect("建环境");

    assert!(!env.id.is_empty(), "空 id 必须被铸掉");
    assert_eq!(read_row(&pool, &env.id).id, env.id);
}
