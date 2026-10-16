//! 落库：结构与 IR 一致、非法放置被拒、重复落库不产生副本、按勾选裁剪。
//!
//! 测试池自己建：core 的 `test_support` 是 `pub(crate)`，跨 crate 不可达，
//! 故这里照 `db::init` 的形状走一遍，得到的是一个没有种子行的真实空库。

use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{uid, HttpMethod, KeyValue, Project, ResourceNodeType};
use simple_post_core::error::AppError;

use super::{commit, CommitReport, CommitTarget};
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule};

pub(super) static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 「勾满 + 落项目顶层」。绝大多数落库测试要的是这个形状，故给一个短名字。
pub(super) fn commit_all(
    pool: &DbPool,
    project_id: &str,
    ex: &Exchange,
) -> Result<CommitReport, AppError> {
    commit(
        pool,
        project_id,
        CommitTarget::ProjectRoot,
        ex,
        &ex.endpoint_keys(),
        true,
    )
}

/// 一个空的临时库 + 一个带默认环境与模块的项目。
pub(super) fn fresh_project() -> (DbPool, String) {
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

pub(super) fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        id: String::new(),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }
}

/// `create_with_defaults` 种下的那个模块。导入的落点用它。
pub(super) fn seeded_module(pool: &DbPool, project_id: &str) -> String {
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
pub(super) fn nested_exchange() -> Exchange {
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
