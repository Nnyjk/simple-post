//! 仅测试。整个文件是 `#[cfg(test)]`，只编进 `cargo test`，别处不存在。
//! `db::init` 没有进这里的路径，故生产构建永远插不进种子行——全新安装是空库。
//! `fixtures.json` 留在仓库里纯粹作为测试数据。

use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;

use super::*;
use crate::domain::{ResourceNode, ResourceNodeType};
use crate::error::AppError;
use rusqlite::params;

/// 内嵌的 fixture 文件。纯测试数据——`db::init` 永不读取。
const FIXTURES_JSON: &str = include_str!("fixtures.json");

/// 解析内嵌 fixture。这里 panic 是正确的：`fixtures.json` 畸形是仓库缺陷，不是运行期状况。
fn fixtures() -> Fixtures {
    serde_json::from_str(FIXTURES_JSON).expect("fixtures.json must parse")
}

/// 在 `dir` 开库并灌入内嵌 fixture 集。
/// 供 `#[path]` 挂载的测试模块使用：播种是显式的一步，而不是 `db::init` 的副作用。
pub(crate) fn init_seeded(dir: &Path) -> DbPool {
    let pool = init(dir).expect("init");
    seed(&pool);
    pool
}

/// 把 fixture 集灌进已初始化的库。
/// 用 `with_conn` 而非 `with_tx`：`seed_fixtures` 自开事务，外层再套一个
/// `BEGIN IMMEDIATE` 会报 "cannot start a transaction within a transaction"。
pub(crate) fn seed(pool: &DbPool) {
    with_conn(pool, |c| seed_fixtures(c, &fixtures())).expect("seed fixtures");
}

/// `fixtures.json` 的顶层形状。只反序列化播种真正需要的字段，文件可增章节而不破旧
/// 二进制（新字段须有默认值）。
#[derive(Debug, Deserialize)]
struct Fixtures {
    /// fixture 集打上的固定时钟，确定性——播种后的库逐字节一致。
    #[serde(default)]
    now: i64,
    #[serde(default)]
    projects: Vec<FixtureProject>,
    #[serde(default)]
    modules: Vec<FixtureModule>,
    #[serde(default)]
    collections: Vec<FixtureCollection>,
    #[serde(default)]
    endpoints: Vec<FixtureEndpoint>,
    #[serde(default)]
    environments: Vec<FixtureEnvironment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureProject {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    base_url_definitions: Vec<FixtureBaseUrlDef>,
    #[serde(default)]
    default_base_url_definition_id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    color: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureBaseUrlDef {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureModule {
    id: String,
    project_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    sort_order: i64,
    /// 仅 UI 的展开状态。带入 fixture 以便渲染测试能读到真实树，永不落库。
    #[serde(default)]
    #[allow(dead_code)]
    expanded: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCollection {
    id: String,
    module_id: String,
    #[serde(default)]
    parent_collection_id: Option<String>,
    name: String,
    #[serde(default)]
    description: Option<String>,
    sort_order: i64,
    /// 仅 UI 的展开状态，见 [`FixtureModule::expanded`]。
    #[serde(default)]
    #[allow(dead_code)]
    expanded: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureEndpoint {
    id: String,
    collection_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    method: String,
    url: String,
    #[serde(default)]
    environment_id: Option<String>,
    #[serde(default)]
    base_url_definition_id: Option<String>,
    #[serde(default)]
    base_url_override: Option<String>,
    #[serde(default)]
    params: Vec<serde_json::Value>,
    #[serde(default)]
    headers: Vec<serde_json::Value>,
    body: serde_json::Value,
    auth: serde_json::Value,
    #[serde(default)]
    docs: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    sort_order: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureEnvironment {
    id: String,
    project_id: String,
    name: String,
    /// `def_id -> module_id -> url`，现役值层。下线的 `base_urls_json` 完全不播种：
    /// fixture 库是现役模型的库，留下有值的旧列会让每个播种测试启动都跑一遍数据迁移。
    #[serde(default)]
    module_base_urls: serde_json::Value,
    #[serde(default)]
    variables: serde_json::Value,
    is_active: bool,
}

/// 插入资源树的一行。fixture 的三个树段落都落到这一种形状，变的只有 `kind` 与
/// `parent_id` 是否为空。收 [`ResourceNode`] 而非散列，是为了让翻译写成几个结构体字面量，
/// 而不是十个位置参数的调用。
fn insert_node(tx: &rusqlite::Transaction<'_>, n: &ResourceNode) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO nodes \
     (id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at) \
     VALUES (?,?,?,?,?,?,?,?,?)",
        params![
            n.id,
            n.root_id,
            n.parent_id,
            n.kind.as_str(),
            n.name,
            n.description,
            n.sort_order,
            n.created_at,
            n.updated_at
        ],
    )?;
    Ok(())
}

/// 按依赖拓扑顺序插入 fixture 全部行：projects → base_url_defs + modules + envs →
/// collections → endpoints。外键自然满足，无需 `PRAGMA defer_foreign_keys`。
fn seed_fixtures(conn: &mut Connection, fx: &Fixtures) -> Result<(), AppError> {
    let tx = conn.transaction()?;

    for p in &fx.projects {
        tx.execute(
            "INSERT INTO projects \
         (id, name, description, default_base_url_definition_id, color, created_at, updated_at) \
         VALUES (?,?,?,?,?,?,?)",
            params![
                p.id,
                p.name,
                p.description,
                p.default_base_url_definition_id,
                p.color,
                p.created_at,
                p.updated_at,
            ],
        )?;
        // 每个 def 单独入表，endpoint 的外键链才成立。
        for (i, d) in p.base_url_definitions.iter().enumerate() {
            tx.execute(
                "INSERT INTO base_url_definitions \
             (id, project_id, name, description, sort_order) \
             VALUES (?,?,?,?,?)",
                params![d.id, p.id, d.name, d.description, i as i64],
            )?;
        }
        // `baseUrl` 是 Project 上的遗留字段，schema 无对应列，不落库。
        let _ = p.base_url.as_ref();
    }

    // fixture 保持三段式（modules / collections / endpoints），便于人读样例树；
    // 翻译在此发生：每项一行 `nodes`，endpoint 另加一行 `endpoint_details`。
    // `root_id` 是 fixture 不带的那个值——原本由所属模块隐含，此处推导并按 owner
    // 记忆化，使查表是一次 map 读，而非每个后代都重走一遍。
    let mut owner: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

    for m in &fx.modules {
        insert_node(
            &tx,
            &ResourceNode {
                id: m.id.clone(),
                root_id: m.project_id.clone(),
                // 模块的父是项目，不是另一个节点。
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: m.name.clone(),
                description: m.description.clone(),
                sort_order: m.sort_order,
                created_at: fx.now,
                updated_at: fx.now,
            },
        )?;
        owner.insert(&m.id, &m.project_id);
    }

    for c in &fx.collections {
        let root_id = owner.get(c.module_id.as_str()).copied().ok_or_else(|| {
            AppError::Validation(format!(
                "fixture collection {} names unknown module {}",
                c.id, c.module_id
            ))
        })?;
        // 嵌套集合的 `parent_id` 是集合而非模块——类型矩阵两者都允许，
        // fixture 用是否给 `parentCollectionId` 表示。
        let parent = c
            .parent_collection_id
            .as_deref()
            .unwrap_or(c.module_id.as_str());
        insert_node(
            &tx,
            &ResourceNode {
                id: c.id.clone(),
                root_id: root_id.to_string(),
                parent_id: Some(parent.to_string()),
                kind: ResourceNodeType::Collection,
                name: c.name.clone(),
                description: c.description.clone(),
                sort_order: c.sort_order,
                created_at: fx.now,
                updated_at: fx.now,
            },
        )?;
        owner.insert(&c.id, root_id);
    }

    for e in &fx.endpoints {
        let root_id = owner
            .get(e.collection_id.as_str())
            .copied()
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "fixture endpoint {} names unknown collection {}",
                    e.id, e.collection_id
                ))
            })?;
        insert_node(
            &tx,
            &ResourceNode {
                id: e.id.clone(),
                root_id: root_id.to_string(),
                parent_id: Some(e.collection_id.clone()),
                kind: ResourceNodeType::Endpoint,
                name: e.name.clone(),
                description: e.description.clone(),
                sort_order: e.sort_order,
                created_at: e.created_at,
                updated_at: e.updated_at,
            },
        )?;
        // 节点与配置一起落：没有 detail 行的 endpoint 打不开。
        tx.execute(
            "INSERT INTO endpoint_details \
         (node_id, method, url, environment_id, base_url_definition_id, \
          base_url_override, params_json, headers_json, body_json, \
          auth_json, docs, notes, tags_json) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                e.id,
                e.method,
                e.url,
                e.environment_id,
                e.base_url_definition_id,
                e.base_url_override,
                serde_json::to_string(&e.params)?,
                serde_json::to_string(&e.headers)?,
                serde_json::to_string(&e.body)?,
                serde_json::to_string(&e.auth)?,
                e.docs,
                e.notes,
                serde_json::to_string(&e.tags)?,
            ],
        )?;
    }

    for env in &fx.environments {
        let module_base_urls_json = serde_json::to_string(&env.module_base_urls)?;
        let variables_json = serde_json::to_string(&env.variables)?;
        tx.execute(
            "INSERT INTO environments \
         (id, project_id, name, is_active, module_base_urls_json, variables_json) \
         VALUES (?,?,?,?,?,?)",
            params![
                env.id,
                env.project_id,
                env.name,
                env.is_active as i64,
                module_base_urls_json,
                variables_json,
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}
