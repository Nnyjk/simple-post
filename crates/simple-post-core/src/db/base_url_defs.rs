//! `base_url_definitions` 表的 CRUD。
//!
//! 服务由**项目**拥有，每个模块都能看到整张列表——用户的规则是「服务应该绑定到项目，
//! 这样每一个模块都会全部包含这些服务」。故此处完全没有按模块过滤：同一项目里两个服务
//! 不可能同名，而 endpoint 的 `base_url_definition_id` 无论 endpoint 挂在哪里都指同一个
//! 服务，这正是模块之间的拖拽无需改指任何东西也能继续工作的原因。
//!
//! 服务解析到的 URL 不在这张表里，它是 `super::environments::module_base_urls` 里的
//! (环境 × 模块 × 服务) 单元格；★ 住在项目上（`projects.default_base_url_definition_id`），
//! 而不在某一行上。
//!
//! `module_id` 与 `is_default` 仍是表里的列，因为带它们的构建曾经发布过，而删列 = 重建表 =
//! bump `SCHEMA_VERSION` = `ensure_current_schema` 删掉用户的库。[`list_by_project`] 与
//! [`get`] 刻意不读它们，此处也没有任何地方写它们。

use rusqlite::{params, Connection, Row};

use crate::domain::BaseUrlDefinition;
use crate::error::AppError;

// 读。

/// 某项目的全部服务定义，按 `sort_order` 排序（使选择器与用户当初设置的顺序一致）。
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<BaseUrlDefinition>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description \
         FROM base_url_definitions \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_def)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<BaseUrlDefinition, AppError> {
    let mut stmt =
        conn.prepare("SELECT id, name, description FROM base_url_definitions WHERE id = ?1")?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_def(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("base_url_definition {id}"))),
    }
}

/// 一个定义所属的项目。调用方需要它来把改动关在所属项目内——`BaseUrlDefinition` 刻意不
/// 带 `project_id`，故这是仅凭定义 id 恢复它的唯一办法。
pub fn project_id_of(conn: &mut Connection, id: &str) -> Result<String, AppError> {
    let pid: Option<String> = conn
        .query_row(
            "SELECT project_id FROM base_url_definitions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    match pid {
        Some(p) => Ok(p),
        None => Err(AppError::NotFound(format!("base_url_definition {id}"))),
    }
}

// 写。

pub fn create(
    conn: &mut Connection,
    def: &BaseUrlDefinition,
    project_id: &str,
) -> Result<(), AppError> {
    // 插入时 `sort_order` 按 schema 取默认 0。渲染层若要重新编号，可用 `reorder`。
    conn.execute(
        "INSERT INTO base_url_definitions (id, project_id, name, description) \
         VALUES (?1, ?2, ?3, ?4)",
        params![def.id, project_id, def.name, def.description],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    name: Option<String>,
    description: Option<Option<String>>,
) -> Result<BaseUrlDefinition, AppError> {
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(n) = name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(n));
    }
    if let Some(d) = description {
        sets.push(format!("description = ?{}", binds.len() + 1));
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v),
            None => rusqlite::types::Value::Null,
        });
    }

    if sets.is_empty() {
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE base_url_definitions SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("base_url_definition {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    // ★ 指着这里的一行，所以删行必须把指针一并带走。留着也不会崩——解析把未知服务当作
    // 「没有 base URL」——但项目会继续宣传一个已不存在的默认，★ 菜单里会渲染出一个幽灵条目
    // 直到有人点了它。一次 `UPDATE`，不做提升：每项目只有一个 ★，没有可提升的对象。
    let mut tx = conn.savepoint()?;
    let removed = tx.execute(
        "DELETE FROM base_url_definitions WHERE id = ?1",
        params![id],
    )?;
    if removed == 0 {
        // 没匹配上：回滚 savepoint 而不是报成功，好让一个打错的 id 像这里其他 CRUD 调用
        // 一样是 NotFound。否则反而是下面的 ★ 更新在决定结果，而只要 ★ 握在别的服务手里
        // 它就一条也匹配不上——恰好就是删除**确实**有活要干的时候。
        tx.rollback()?;
        return Err(AppError::NotFound(format!("base_url_definition {id}")));
    }
    tx.execute(
        "UPDATE projects SET default_base_url_definition_id = NULL \
         WHERE default_base_url_definition_id = ?1",
        params![id],
    )?;
    tx.commit()?;
    Ok(())
}

/// 把 `id` 标为项目的 ★ 服务。只更新 `projects.default_base_url_definition_id`；
/// 环境与其他定义一概不动。
///
/// ★ 是项目范围而非行范围的：服务列表由每个模块共享，逐服务的标记会让「默认」对不同模块
/// 意味着不同的东西。未指名服务的 endpoint 所解析的对象，是项目 ★，然后是它在环境里自己
/// 那个模块的单元格——解析顺序见设置模型。
/// 传 `def_id = None` 即清除默认。
pub fn set_default(
    conn: &mut Connection,
    project_id: &str,
    def_id: Option<&str>,
) -> Result<(), AppError> {
    let rows = conn.execute(
        "UPDATE projects SET default_base_url_definition_id = ?1 WHERE id = ?2",
        params![def_id, project_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("project {project_id}")));
    }
    Ok(())
}

// 行映射器。

fn row_to_def(row: &Row<'_>) -> Result<BaseUrlDefinition, rusqlite::Error> {
    Ok(BaseUrlDefinition {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
    })
}

// 测试。

#[cfg(test)]
#[path = "base_url_defs_test.rs"]
mod tests;
