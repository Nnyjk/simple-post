//! `projects` 表的 CRUD。
//!
//! 删除路径依赖 `schema.sql` 里的 `ON DELETE CASCADE`——删一个项目会传递地移除它的模块、
//! collection、endpoint、环境、base_url_definitions 以及那条 agent 权限行，故外键保持
//! 干净，无需显式清理循环。
//!
//! `Project.base_url_definitions` 住在 `base_url_definitions` 表里（分开是为了拿到外键
//! 级联与自己的索引）。读取时用第二个以 project_id 为键的查询把它填上。

use rusqlite::{params, Connection, Row};

use crate::domain::{Project, ProjectPatch};
use crate::error::AppError;

// 读。

/// 全部项目，并填好各自的 `base_url_definitions`。两条查询——一条取项目行，一条按
/// project_id 分组取全部定义——故代价不随每个项目有几个定义而变。
pub fn list(conn: &mut Connection) -> Result<Vec<Project>, AppError> {
    // 先收集项目，好让 `stmt` 对 `conn` 的借用在为定义 join 打开第二条语句之前就结束。
    let mut projects = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, default_base_url_definition_id, color, \
                    created_at, updated_at \
             FROM projects \
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], row_to_project)?;
        for r in rows {
            projects.push(r.map_err(AppError::Sqlite)?);
        }
    }
    populate_defs(conn, &mut projects)?;
    Ok(projects)
}

/// 单个项目查询，并填好其 `base_url_definitions`。id 不存在时为 `AppError::NotFound`。
pub fn get(conn: &mut Connection, id: &str) -> Result<Project, AppError> {
    let project = {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, default_base_url_definition_id, color, \
                    created_at, updated_at \
             FROM projects WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => row_to_project(row).map_err(AppError::Sqlite)?,
            None => return Err(AppError::NotFound(format!("project {id}"))),
        }
    }; // stmt + rows 在此析构，释放对 `conn` 的不可变借用。
    let mut single = vec![project];
    populate_defs(conn, &mut single)?;
    Ok(single.remove(0))
}

// 写。

/// 插入一个项目。`created_at` / `updated_at` 取自调用方给的结构体；不在服务端 `now()`，
/// 因为渲染层有时会为 offline-first 流程种下乐观时间戳。
/// 注意：`Project` 上的 `base_url_definitions` 字段在此**刻意**被忽略——base URL 定义住在
/// 自己的表里，必须经 `base_url_defs::create` 插入。想连定义一起建项目的调用方应把两次
/// 插入放进同一个事务。
pub fn create(conn: &mut Connection, p: &Project) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO projects \
         (id, name, description, default_base_url_definition_id, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
    Ok(())
}

/// 施加部分更新。只写 `Some(...)` 的字段；`Option<Option<T>>`（`description`、
/// `default_base_url_definition_id`）区分「保持不动」（外层 `None`）与「清空」（内层 `None`）。
/// 返回更新后的行，好让调用方无需额外 `get` 即可回显给渲染层。
pub fn update(conn: &mut Connection, id: &str, patch: ProjectPatch) -> Result<Project, AppError> {
    // 收集自有绑定的值（以便能越过 `patch` 借用它们的那一段存活在 Vec 里）与平行的 SET 子句
    // 列表，真正的 SQL 最后再拼。
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(name.clone()));
    }
    if let Some(d) = &patch.description {
        sets.push(format!("description = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    // `patch.base_url` 收下但**忽略**。`projects` 表从未有过 `base_url` 列——项目的 URL 住在
    // 某个环境下某模块的服务的 `moduleBaseUrls`——为它发出 `SET base_url = ?` 会把
    // `PATCH {"baseUrl": "…"}` 变成一个 `no such column` 错误，即把调用方的错误说成数据库
    // 故障。把该字段留在 patch 类型里可保住旧客户端载荷的序列化，静默丢弃是代价，
    // `domain.rs` 里该字段的注释也是这么写的。
    if let Some(d) = &patch.default_base_url_definition_id {
        sets.push(format!(
            "default_base_url_definition_id = ?{}",
            binds.len() + 1
        ));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(c) = &patch.color {
        sets.push(format!("color = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(c.clone()));
    }
    // 总是推进 updated_at。
    sets.push(format!("updated_at = ?{}", binds.len() + 1));
    binds.push(rusqlite::types::Value::Integer(crate::domain::now_ms()));

    let sql = format!(
        "UPDATE projects SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));

    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    get(conn, id)
}

/// 删除一个项目。schema 里的级联一次性带走模块、collection、endpoint、环境、
/// base_url_defs 与权限行。id 不存在时返回 `NotFound`，好让调用方自己决定静默成功还是 404。
pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {id}")));
    }
    Ok(())
}

/// 每个新项目出生时自带的环境。
///
/// 零环境的项目是死路而不是白纸：`{{var}}` 无从解析，每个 base URL 都解析成 `''`，设置面板
/// 打开就是一句让用户去新建的空状态提示。种下第一行意味着用户紧接着做的下一件事——加一个
/// 请求——就已经能用它。可就地改名，故这个名字是起点而非决定。
pub const DEFAULT_ENVIRONMENT_NAME: &str = "默认环境";

/// 每个新项目出生时自带的模块。
///
/// 模块一度**刻意**不种：一个用户没要的「默认模块」读起来像一行他得注意到并删掉的东西。
/// 待模块成了 base URL 表格的行轴**并且**成了工作区 服务 区的主题，天平又倒向了另一边。
/// 没有它，一个全新项目会同时摆出三个死路——树的「该项目还没有模块」、前置 URL 表格的
/// 「没有模块可归属」、以及空的服务区——而这是一张用户反正会用到的形状。种下它把三件事并成
/// 一件要填的事。可就地改名，与环境同一契约。
pub const DEFAULT_MODULE_NAME: &str = "默认模块";

/// 创建一个项目**以及**让它可用所必需的那两行。
///
/// 单独调用 [`create`] 只拿到项目行本身，故它写出的项目既无环境也无模块——创建对话框调用方
/// 拿到的正是那个状态。
///
/// 种了什么，以及同样重要的、没种什么：
/// - 总是种 [`DEFAULT_ENVIRONMENT_NAME`]，且为激活态，故项目一存在，`{{var}}` 与每个
///   base URL 就有地方解析。
/// - 总是种 [`DEFAULT_MODULE_NAME`]，故树、base URL 表格与模块工作区都有行可指。
/// - **不种服务，不种 URL。** 服务列表属于项目，而 URL 属于 (环境 × 模块 × 服务)，故 URL
///   需要一个用户还没起的服务名。种一个占位的「默认服务」和它的 URL 槽位，等于凭空造一个
///   他随后还得去找并删掉的名字；而在创建表单里问 URL，是问一个无处安放的值。用户在设置的
///   笛卡尔网格里填它，那里他正在填的那一行是看得见的。
///
/// 返回的 `Project` 是写入后的读结果，调用方可直接回传给渲染层而无需重新拉取。
/// 调用方必须已持有事务（`db::with_tx`）：项目、它的环境与它的模块必须一起落地，否则都不该落。
pub fn create_with_defaults(conn: &mut Connection, project: &Project) -> Result<Project, AppError> {
    let mut project = project.clone();
    // 调用方的结构体可能带着为表单准备的乐观状态（对重新提交的表单，还有一个乐观的 ★）。
    // 既然什么都没种，诚实的答案就是空服务列表且无默认——把调用方的值回显会让响应与实际写入
    // 的内容不一致。
    project.base_url_definitions = Vec::new();
    project.default_base_url_definition_id = None;

    create(conn, &project)?;

    let env = crate::domain::Environment {
        id: crate::domain::uid(),
        project_id: project.id.clone(),
        name: DEFAULT_ENVIRONMENT_NAME.to_string(),
        base_urls: Default::default(),
        module_base_urls: Default::default(),
        variables: Default::default(),
        disabled_variables: Default::default(),
        is_active: true,
    };
    super::environments::create(conn, &env)?;

    let module = crate::domain::ResourceNode {
        id: crate::domain::uid(),
        root_id: project.id.clone(),
        // 模块的父是项目本身。`nodes` 里没有项目行，故顶层模块的父存为 `NULL`，由 `root_id` 说明
        // 它归哪个项目所有。
        parent_id: None,
        kind: crate::domain::ResourceNodeType::Module,
        name: DEFAULT_MODULE_NAME.to_string(),
        description: None,
        // 第一行也是唯一一行，故顺序无所谓——0 就是「创建模块」那条路径给首个模块写的值。
        sort_order: 0,
        created_at: crate::domain::now_ms(),
        updated_at: crate::domain::now_ms(),
    };
    super::nodes::create(conn, &module, None)?;

    Ok(project)
}

// 行映射器。

fn row_to_project(row: &Row<'_>) -> Result<Project, rusqlite::Error> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        // 不落库；仅遗留迁移使用
        base_url: None,
        // 由 populate_defs 填充
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: row.get(3)?,
        color: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// 为输入向量里的每个项目从 join 表填入 `base_url_definitions`。无论项目多少，总共两条
/// 查询：一条取全部定义，然后在 Rust 里做一次 hashmap join。
fn populate_defs(conn: &mut Connection, projects: &mut [Project]) -> Result<(), AppError> {
    if projects.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, description \
         FROM base_url_definitions \
         WHERE project_id IN (SELECT id FROM projects) \
         ORDER BY project_id ASC, sort_order ASC, id ASC",
    )?;
    // 按 project_id 分桶。数据集很小（总共几十条定义），HashMap 有点大材小用，但代码路径更直白。
    let mut by_project: std::collections::HashMap<String, Vec<crate::domain::BaseUrlDefinition>> =
        std::collections::HashMap::new();
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let project_id: String = row.get(1)?;
        let name: String = row.get(2)?;
        let description: Option<String> = row.get(3)?;
        Ok((
            project_id,
            crate::domain::BaseUrlDefinition {
                id,
                name,
                description,
            },
        ))
    })?;
    for r in rows {
        let (pid, def) = r?;
        by_project.entry(pid).or_default().push(def);
    }
    for p in projects.iter_mut() {
        if let Some(defs) = by_project.remove(&p.id) {
            p.base_url_definitions = defs;
        }
    }
    Ok(())
}

// 测试。

#[cfg(test)]
#[path = "projects_test.rs"]
mod tests;
