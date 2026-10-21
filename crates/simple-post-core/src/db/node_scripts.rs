//! `node_scripts` 表的 CRUD —— collection 节点上的两段脚本。
//!
//! 端点的脚本住在 `endpoint_details`（与 body / headers 同级），collection 的脚本住这里。
//! 两者线上形状相同，故共用同一个 [`NodeScripts`]。
//!
//! **只有 collection 能有脚本**：模块不需要脚本，endpoint 走 `endpoint_details`。
//! 应用层查一次节点类型、SQLite 触发器再查一次——与父子矩阵同样的两道防线。

use rusqlite::{params, Connection, Row};

use crate::domain::{NodeScripts, NodeScriptsPatch};
use crate::error::AppError;

/// 读一个节点的脚本。
///
/// **没有行不是错误**：没配过脚本是正常状态，于是返回一份全空的 `NodeScripts`。
/// 这一点与 `endpoint_details` 相反——那里「节点存在却没有 detail 行」意味着两张表
/// 不一致，值得暴露出来。
pub fn get(conn: &mut Connection, node_id: &str) -> Result<NodeScripts, AppError> {
    let sql =
        "SELECT node_id, pre_request_script, test_script FROM node_scripts WHERE node_id = ?1";
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![node_id])?;
    match rows.next()? {
        Some(row) => row_to_scripts(row).map_err(AppError::Sqlite),
        None => Ok(empty(node_id)),
    }
}

/// 写一个节点的脚本。
///
/// patch 的三态：缺席 = 不动，`null` = 清除，设值 = 写入。
pub fn set(
    conn: &mut Connection,
    node_id: &str,
    patch: NodeScriptsPatch,
) -> Result<NodeScripts, AppError> {
    ensure_collection(conn, node_id)?;

    let current = get(conn, node_id)?;
    let pre_request = match &patch.pre_request_script {
        Some(value) => value.clone(),
        None => current.pre_request_script,
    };
    let test = match &patch.test_script {
        Some(value) => value.clone(),
        None => current.test_script,
    };

    if pre_request.is_none() && test.is_none() {
        // 两段都清空了：把行删掉，免得留下一个永远读成「没有脚本」的空行。
        conn.execute(
            "DELETE FROM node_scripts WHERE node_id = ?1",
            params![node_id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO node_scripts (node_id, pre_request_script, test_script) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(node_id) DO UPDATE SET \
               pre_request_script = ?2, test_script = ?3",
            params![node_id, pre_request, test],
        )?;
    }
    get(conn, node_id)
}

/// 节点必须是 collection。
fn ensure_collection(conn: &mut Connection, node_id: &str) -> Result<(), AppError> {
    let kind: String = match conn.query_row(
        "SELECT type FROM nodes WHERE id = ?1",
        params![node_id],
        |row| row.get(0),
    ) {
        Ok(kind) => kind,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(AppError::NotFound(format!("node {node_id}")));
        }
        Err(e) => return Err(AppError::Sqlite(e)),
    };
    if kind == "collection" {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "只有 collection 能带脚本（节点 {node_id} 是 {kind}）"
    )))
}

fn empty(node_id: &str) -> NodeScripts {
    NodeScripts {
        node_id: node_id.to_string(),
        pre_request_script: None,
        test_script: None,
    }
}

/// 一个项目里所有集合的脚本。
///
/// **一次读回整棵树的脚本**，与 [`super::endpoint_details::list_by_root`] 同一条纪律：
/// 导出是一次投影，逐节点发查询会让一棵大树变成 N 次往返。
///
/// 只有**有脚本**的节点在结果里——没配过脚本是正常状态，不该占一行。
pub fn list_by_root(conn: &mut Connection, root_id: &str) -> Result<Vec<NodeScripts>, AppError> {
    let sql = "SELECT s.node_id, s.pre_request_script, s.test_script \
               FROM node_scripts s \
               JOIN nodes n ON n.id = s.node_id \
               WHERE n.root_id = ?1 \
               ORDER BY n.sort_order ASC, s.node_id ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![root_id], row_to_scripts)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

fn row_to_scripts(row: &Row<'_>) -> Result<NodeScripts, rusqlite::Error> {
    Ok(NodeScripts {
        node_id: row.get(0)?,
        pre_request_script: row.get(1)?,
        test_script: row.get(2)?,
    })
}

// 测试。

#[cfg(test)]
#[path = "node_scripts_test.rs"]
mod tests;
