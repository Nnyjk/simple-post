//! `agent_permissions` 表的 CRUD。
//!
//! 存放授予 MCP / 外部 agent 的逐项目权限。每项目一行；行缺失时默认
//! [`Permission::ReadCall`]。注意此处仅为项目级权限门，实际生效时与全局
//! `mcp.policy`（默认 `read_only`）取更严格档位，故未显式放开全局策略前 AI 仍不可写。

use rusqlite::{params, Connection};

use crate::domain::Permission;
use crate::error::AppError;

/// 取 `project_id` 的权限。行不存在时为 `ReadCall`，与渲染层在启动时「在被告知之前从宽」
/// 的立场一致。
pub fn get(conn: &mut Connection, project_id: &str) -> Result<Permission, AppError> {
    let perm: Option<String> = conn
        .query_row(
            "SELECT permission FROM agent_permissions WHERE project_id = ?1",
            params![project_id],
            |r| r.get(0),
        )
        .ok();
    match perm {
        Some(s) => Permission::parse(&s)
            .ok_or_else(|| AppError::Db(format!("unknown permission value in DB: {s:?}"))),
        None => Ok(Permission::default()),
    }
}

/// Upsert 权限。使用 INSERT ... ON CONFLICT(project_id) DO UPDATE。
/// 注意：函数不主动校验项目是否存在；若传入不存在的 `project_id`，底层外键约束
/// 失败将返回 `AppError::Db(...)`。
pub fn set(conn: &mut Connection, project_id: &str, perm: Permission) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO agent_permissions (project_id, permission) \
         VALUES (?1, ?2) \
         ON CONFLICT(project_id) DO UPDATE SET permission = excluded.permission",
        params![project_id, perm.as_str()],
    )?;
    Ok(())
}

/// 移除 `project_id` 的覆盖。此后 `get` 退回默认的 `ReadCall`。
pub fn clear(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM agent_permissions WHERE project_id = ?1",
        params![project_id],
    )?;
    Ok(())
}

// 测试。

#[cfg(test)]
#[path = "permissions_test.rs"]
mod tests;
