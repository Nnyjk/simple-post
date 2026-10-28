//! `domain → ir → domain` 往返与「不回退」的服务取值。

use std::sync::atomic::{AtomicU64, Ordering};

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    uid, AuthConfig, AuthType, BaseUrlDefinition, BodyMode, EndpointDetail, HttpMethod, KeyValue,
    Project, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

pub(super) static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 一个项目：自带一个默认模块、一个激活环境，外加一条服务定义（名「网关」）。
pub(super) struct Seeded {
    pub(super) project_id: String,
    pub(super) module_id: String,
    pub(super) env_id: String,
    pub(super) def_id: String,
}

pub(super) fn temp_pool(tag: &str) -> DbPool {
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
pub(super) fn seed_project(pool: &DbPool) -> Seeded {
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
        path_params: vec![],
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
        pre_request_script: None,
        test_script: None,
        tls: None,
    }
}

pub(super) fn add_endpoint(pool: &DbPool, s: &Seeded, parent: &str) -> String {
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

/// 一个**没有** detail 行的 endpoint 节点。
///
/// 两张表由不同事务写入，正常路径下不会不一致，故这里直接造出那个状态：先照常
/// 建出节点与它的 detail 行，再把 detail 行删掉。`db::nodes::create` 本身拒绝
/// 「endpoint 没有 detail」（那是另一道防线），故只能这样造。
pub(super) fn add_endpoint_without_detail(pool: &DbPool, s: &Seeded, parent: &str) -> String {
    let id = add_endpoint(pool, s, parent);
    db::with_tx(pool, |conn| {
        conn.execute("DELETE FROM endpoint_details WHERE node_id = ?1", [&id])?;
        Ok::<(), AppError>(())
    })
    .expect("delete detail row");
    id
}

pub(super) fn add_module(pool: &DbPool, project_id: &str, name: &str) -> String {
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
