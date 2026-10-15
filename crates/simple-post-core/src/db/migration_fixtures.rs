// 数据迁移测试用的 fixture 构造与解析助手——两种前任服务表形状共用。
//
// 下面的库是用 `init` 建的（对空库跑 `migrate`，无操作），再按每个前任构建各自的写法填入行。
// 这对真实的旧文件是忠实的替身：迁移读的是行，而当前的 `CREATE TABLE` 带着两个前任都有的
// 每一列——`module_id` 与 `is_default` 也在内——故唯一区分「旧文件」的就是里面的数据，
// 而那正是这些测试所设置的。

use rusqlite::{params, Connection};

use super::test_helpers::temp_dir;
use super::*;

/// 模块 → URL map，两个 `environments` JSON 列共用的形状。
pub(crate) type UrlMap = std::collections::BTreeMap<String, String>;
/// def_id → module_id → URL。
pub(crate) type PerModuleMap = std::collections::BTreeMap<String, UrlMap>;

/// 当前 schema 且空无一行的库，等着放旧行。数据迁移在此已经跑过一次（对零行），这正是
/// 一个启动中的应用被交上一个必须修复的数据库时所处的状态。
pub(crate) fn legacy_store(tag: &str) -> DbPool {
    let dir = temp_dir(tag);
    init(&dir).expect("init")
}

fn add_project(conn: &mut Connection, id: &str, star: Option<&str>) {
    conn.execute(
        "INSERT INTO projects (id, name, default_base_url_definition_id, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, '#60a5fa', 0, 0)",
        params![id, id, star],
    )
    .unwrap();
}

fn add_module(conn: &mut Connection, id: &str, project: &str, sort_order: i64, created_at: i64) {
    conn.execute(
        "INSERT INTO nodes (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, NULL, 'module', ?1, ?3, ?4, ?4)",
        params![id, project, sort_order, created_at],
    )
    .unwrap();
}

/// 一条服务行。`module` 正是区分两种形状的东西：`Some` 是模块持有布局，`None` 是项目持有。
fn add_service(
    conn: &mut Connection,
    id: &str,
    project: &str,
    name: &str,
    module: Option<&str>,
    sort_order: i64,
    is_default: bool,
) {
    conn.execute(
        "INSERT INTO base_url_definitions \
         (id, project_id, module_id, name, sort_order, is_default) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, project, module, name, sort_order, is_default as i64],
    )
    .unwrap();
}

fn add_env(
    conn: &mut Connection,
    id: &str,
    project: &str,
    base_urls: &UrlMap,
    module_base_urls: &PerModuleMap,
) {
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, is_active, base_urls_json, module_base_urls_json) \
         VALUES (?1, ?2, ?1, 1, ?3, ?4)",
        params![
            id,
            project,
            serde_json::to_string(base_urls).unwrap(),
            serde_json::to_string(module_base_urls).unwrap()
        ],
    )
    .unwrap();
}

/// 直接挂在 `module` 下的 endpoint——树允许这样，且它让这些 fixture 每个 endpoint 只用一个节点。
fn add_endpoint(conn: &mut Connection, id: &str, module: &str, service: Option<&str>, path: &str) {
    conn.execute(
        "INSERT INTO nodes (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
         VALUES (?1, (SELECT root_id FROM nodes WHERE id = ?2), ?2, 'endpoint', ?1, 0, 0, 0)",
        params![id, module],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO endpoint_details (node_id, method, url, base_url_definition_id) \
         VALUES (?1, 'GET', ?2, ?3)",
        params![id, path, service],
    )
    .unwrap();
}

/// 某 endpoint 挂在哪个模块下。沿树上溯，因为 endpoint 至少比它的模块低一条边，而中间夹
/// 一个 collection 不该终止这次上溯。
fn module_of(conn: &mut Connection, node_id: &str) -> Option<String> {
    conn.query_row(
        "WITH RECURSIVE up(id, parent_id, type) AS ( \
             SELECT id, parent_id, type FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT p.id, p.parent_id, p.type FROM nodes p JOIN up ON p.id = up.parent_id \
         ) \
         SELECT id FROM up WHERE type = 'module' LIMIT 1",
        params![node_id],
        |r| r.get(0),
    )
    .ok()
}

pub(crate) fn endpoint_service(conn: &mut Connection, node_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT base_url_definition_id FROM endpoint_details WHERE node_id = ?1",
        params![node_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .unwrap()
}

pub(crate) fn project_star(conn: &mut Connection, project_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT default_base_url_definition_id FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )
    .unwrap()
}

pub(crate) fn read_env_maps(conn: &mut Connection, env_id: &str) -> (UrlMap, PerModuleMap) {
    conn.query_row(
        "SELECT base_urls_json, module_base_urls_json FROM environments WHERE id = ?1",
        params![env_id],
        |r| {
            Ok((
                serde_json::from_str::<UrlMap>(&r.get::<_, String>(0)?).unwrap(),
                serde_json::from_str::<PerModuleMap>(&r.get::<_, String>(1)?).unwrap(),
            ))
        },
    )
    .unwrap()
}

/// **当前**模型如何解析一个 endpoint 的 base URL：endpoint 未指名服务时用项目的 ★，然后是
/// 该服务在 endpoint 自己模块下的单元格，别无其他。
fn resolve_current(
    conn: &mut Connection,
    project_id: &str,
    env_id: &str,
    endpoint_id: &str,
) -> String {
    let def = endpoint_service(conn, endpoint_id).or_else(|| project_star(conn, project_id));
    let Some(def) = def else {
        return String::new();
    };
    let Some(module) = module_of(conn, endpoint_id) else {
        return String::new();
    };
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM base_url_definitions WHERE id = ?1 AND project_id = ?2)",
            params![def, project_id],
            |r| r.get(0),
        )
        .unwrap();
    if !exists {
        return String::new();
    }
    read_env_maps(conn, env_id)
        .1
        .get(&def)
        .and_then(|per_module| per_module.get(&module))
        .cloned()
        .unwrap_or_default()
}

/// **shape A**（项目持有列表）布局当年如何解析：先按模块覆盖，再环境级 URL。
pub(crate) fn resolve_legacy_project_owned(
    conn: &mut Connection,
    project_id: &str,
    endpoint_id: &str,
    env_id: &str,
) -> String {
    let def = endpoint_service(conn, endpoint_id).or_else(|| project_star(conn, project_id));
    let Some(def) = def else {
        return String::new();
    };
    let Some(module) = module_of(conn, endpoint_id) else {
        return String::new();
    };
    let (base_urls, overrides) = read_env_maps(conn, env_id);
    overrides
        .get(&def)
        .and_then(|per_module| per_module.get(&module))
        .or_else(|| base_urls.get(&def))
        .cloned()
        .unwrap_or_default()
}

/// **shape B**（模块持有列表）布局当年如何解析：endpoint 指名的服务，或它自己模块的 ★，
/// 直接取自环境级 map。
pub(crate) fn resolve_legacy_module_owned(
    conn: &mut Connection,
    endpoint_id: &str,
    env_id: &str,
) -> String {
    let module = module_of(conn, endpoint_id);
    let def = endpoint_service(conn, endpoint_id).or_else(|| {
        let module = module.clone()?;
        conn.query_row(
            "SELECT id FROM base_url_definitions \
             WHERE module_id = ?1 AND is_default = 1 \
             ORDER BY sort_order ASC, id ASC LIMIT 1",
            params![module],
            |r| r.get(0),
        )
        .ok()
    });
    let Some(def) = def else {
        return String::new();
    };
    read_env_maps(conn, env_id)
        .0
        .get(&def)
        .cloned()
        .unwrap_or_default()
}

/// 一个项目下的 endpoint，以及每种布局各自会解析出的 URL。
pub(crate) fn urls_under_current(
    conn: &mut Connection,
    project_id: &str,
    env_id: &str,
) -> Vec<(String, String)> {
    let ids = {
        let mut stmt = conn
            .prepare("SELECT id FROM nodes WHERE root_id = ?1 AND type = 'endpoint' ORDER BY id")
            .unwrap();
        let ids = stmt
            .query_map(params![project_id], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        ids
    };
    ids.into_iter()
        .map(|id| {
            let url = resolve_current(conn, project_id, env_id, &id);
            (id, url)
        })
        .collect()
}

/// 迁移被允许触碰的一切，以原始字符串给出，好让两次运行能逐字节比对。
pub(crate) fn snapshot(conn: &mut Connection, project_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, module_id, name, sort_order, is_default FROM base_url_definitions \
                 WHERE project_id = ?1 ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "def|{}|{:?}|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, base_urls_json, module_base_urls_json FROM environments \
                 WHERE project_id = ?1 ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "env|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT e.node_id, IFNULL(e.base_url_definition_id, '-') \
                 FROM endpoint_details e JOIN nodes n ON n.id = e.node_id \
                 WHERE n.root_id = ?1 ORDER BY e.node_id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "ep|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    out.push(format!("star|{:?}", project_star(conn, project_id)));
    out
}

/// 真实的 pre-v1 布局：每项目一份服务列表（`module_id` 为 NULL），值在
/// `base_urls_json[def_id]`，按模块的覆盖在 `module_base_urls_json[def_id][module_id]`。
pub(crate) fn seed_shape_a(conn: &mut Connection) {
    add_project(conn, "p_a", Some("d_base"));
    // id 与时间戳以用户自己的库为模型：相隔几分钟创建的两个模块，`sort_order` 0 与 1。
    add_module(conn, "m_air", "p_a", 0, 1_791_617_412_693);
    add_module(conn, "m_water", "p_a", 1, 1_791_621_861_037);
    add_service(conn, "d_base", "p_a", "base", None, 0, false);
    add_service(conn, "d_air", "p_a", "air", None, 1, false);

    let base_urls: UrlMap = [("d_base".to_string(), "http://127.0.0.1:9200".to_string())]
        .into_iter()
        .collect();
    let overrides: PerModuleMap = [(
        "d_base".to_string(),
        [("m_air".to_string(), "http://127.0.0.1:9201".to_string())]
            .into_iter()
            .collect(),
    )]
    .into_iter()
    .collect();
    add_env(conn, "e_a_prod", "p_a", &base_urls, &overrides);

    add_endpoint(conn, "a_named_air", "m_air", Some("d_base"), "/air/health");
    add_endpoint(
        conn,
        "a_named_water",
        "m_water",
        Some("d_base"),
        "/water/health",
    );
    // 未指名服务：这些走项目 ★ 解析，放进来是为了证明那条路径也活着。
    add_endpoint(conn, "a_star_air", "m_air", None, "/star/air");
    add_endpoint(conn, "a_star_water", "m_water", None, "/star/water");
    // 一个任何环境都没给过值的服务：迁移前后答案都是空，且该单元格必须保持缺席，
    // 而不是变成空字符串。
    add_endpoint(conn, "a_unset", "m_air", Some("d_air"), "/air/unset");
}

/// 模块持有构建留下的布局，以一个真实用户库为模型：模块 `air` 持有 `air` / `base`(★) /
/// 「新服务」，模块 `water` 只持有「新服务」(★)，而环境的 URL map 以各模块各自创建的
/// clone 为键。
pub(crate) fn seed_shape_b(conn: &mut Connection) {
    add_project(conn, "p_b", None);
    add_module(conn, "m_air", "p_b", 0, 1_791_617_412_693);
    add_module(conn, "m_water", "p_b", 1, 1_791_621_861_037);

    add_service(conn, "sp_air_air", "p_b", "air", Some("m_air"), 0, false);
    add_service(conn, "sp_air_base", "p_b", "base", Some("m_air"), 1, true);
    add_service(conn, "sp_air_new", "p_b", "新服务", Some("m_air"), 2, false);
    add_service(
        conn,
        "sp_water_new",
        "p_b",
        "新服务",
        Some("m_water"),
        0,
        true,
    );

    // 两个现役 URL，外加真实库会积累的孤儿键：在 UI 里被删掉、已解析不出任何东西的服务 id。
    let base_urls: UrlMap = [
        (
            "sp_air_base".to_string(),
            "http://127.0.0.1:9200".to_string(),
        ),
        (
            "sp_water_new".to_string(),
            "http://127.0.0.1:9300".to_string(),
        ),
        ("sp_deleted_1".to_string(), String::new()),
        ("sp_deleted_2".to_string(), String::new()),
    ]
    .into_iter()
    .collect();
    add_env(conn, "e_b_prod", "p_b", &base_urls, &PerModuleMap::new());

    add_endpoint(
        conn,
        "b_named_air",
        "m_air",
        Some("sp_air_base"),
        "/air/health",
    );
    add_endpoint(
        conn,
        "b_named_water",
        "m_water",
        Some("sp_water_new"),
        "/water/health",
    );
    add_endpoint(conn, "b_unset", "m_air", Some("sp_air_air"), "/air/unset");
}
