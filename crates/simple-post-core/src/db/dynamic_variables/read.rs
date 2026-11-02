//! 读路径：列表、单行读取、按名字读取、归属项目与数据源项目。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::DynamicVariable;
use crate::error::AppError;

use super::mapping::{row_to_var, COLUMNS};

/// 列出全部定义，按 `sort_order` 排序。
///
/// 跨全部项目：这条读路径服务的是「列出工作区里所有动态变量」这种调用（跨项目调用
/// 接口要靠它），项目内的那一份走 [`list_by_project`]。
///
/// `source_endpoint_name` 由一次 LEFT JOIN 顺带取回：界面要显示「数据源：站点清单」，
/// 让调用方自己再查一次树是多余的往返。
pub fn list(conn: &mut Connection) -> Result<Vec<DynamicVariable>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         ORDER BY v.sort_order ASC, v.id ASC",
        COLUMNS
    ))?;
    let rows = stmt.query_map([], row_to_var)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 列出某个项目里的定义，按 `sort_order` 排序。
///
/// 项目内的面板与项目内的取值都用这条：变量名在项目内唯一，故这个列表就是那个项目
/// 全部的动态变量。
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<DynamicVariable>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         WHERE v.project_id = ?1 \
         ORDER BY v.sort_order ASC, v.id ASC",
        COLUMNS
    ))?;
    let rows = stmt.query_map(params![project_id], row_to_var)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 读一条定义。读不到即 `NotFound`——调用方需要区分「没有这条」与「这条坏了」。
pub fn get(conn: &mut Connection, id: &str) -> Result<DynamicVariable, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         WHERE v.id = ?1",
        COLUMNS
    ))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_var(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("dynamic_variable {id}"))),
    }
}

/// 按名字读某个项目里的一条定义。解析阶段用它把 `{{sns}}` 里的名字对上定义。
///
/// **只认这个项目**：同名变量在别的项目里另有一条定义，与这条无关。取值不回退——
/// 消费方项目里没有这个名字，它就是缺失的。
pub fn get_by_name_in_project(
    conn: &mut Connection,
    project_id: &str,
    name: &str,
) -> Result<Option<DynamicVariable>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         WHERE v.project_id = ?1 AND v.name = ?2",
        COLUMNS
    ))?;
    let mut rows = stmt.query(params![project_id, name])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_var(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

/// 按名字读**跨全部项目**的同名定义。
///
/// 跨项目调用接口用它：同一个名字在多个项目里都有定义时，调用方要自己决定用哪一条，
/// 故这里把全部候选都返回，由上层判歧义。
pub fn get_by_name_across(
    conn: &mut Connection,
    name: &str,
) -> Result<Vec<DynamicVariable>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         WHERE v.name = ?1 \
         ORDER BY v.project_id ASC, v.id ASC",
        COLUMNS
    ))?;
    let rows = stmt.query_map(params![name], row_to_var)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 一条定义的归属项目。`None` = 未归属（迁移遗留的定义，等用户认领）。
pub fn project_id_of(conn: &mut Connection, id: &str) -> Result<Option<String>, AppError> {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT project_id FROM dynamic_variables WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    match row {
        None => Err(AppError::NotFound(format!("dynamic_variable {id}"))),
        Some(project_id) => Ok(project_id),
    }
}

/// 一条定义的数据源端点所属的项目。数据源是内联请求时没有项目。
///
/// MCP 工具用它决定「这次取数算在哪个项目的权限下」——取数会真的用到那个项目的
/// base URL 与鉴权。归属项目是另一回事（见 [`project_id_of`]）：那条决定「谁拥有这条
/// 定义」，这条决定「这次取数花谁的钱」。
pub fn source_project_id_of(conn: &mut Connection, id: &str) -> Result<Option<String>, AppError> {
    let endpoint_id: Option<Option<String>> = conn
        .query_row(
            "SELECT source_endpoint_id FROM dynamic_variables WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(endpoint_id) = endpoint_id else {
        return Err(AppError::NotFound(format!("dynamic_variable {id}")));
    };
    // 列为空 = 数据源是内联请求（或端点已被删），故没有项目。
    let Some(endpoint_id) = endpoint_id else {
        return Ok(None);
    };
    // 端点已被删：外键把这一列置成了 NULL，故这里拿不到项目。
    //
    // 别的错误（数据库或连接池出问题）不能读成「没有项目」——调用方靠这个返回值决定
    // 要不要过按项目的权限门，读错等于把门关掉。
    match crate::db::nodes::root_id_of(conn, &endpoint_id) {
        Ok(root) => Ok(Some(root)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(other) => Err(other),
    }
}
