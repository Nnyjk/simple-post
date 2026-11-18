//! `dynamic_variable_cache` 表的 CRUD —— 动态变量的当前值。
//!
//! 一行 = 一个（定义 × 环境）。缓存键含环境：同一个定义在不同环境取到的值不同——数据源
//! 端点的 base URL 随环境变，测试环境与生产环境的站点清单本来就不是一回事。没有环境时
//! `environment_id` 用空字符串占位。
//!
//! 表里存的是**抽到的原始值**（JSON 文本），不是序列化后的形态：形态由定义的
//! `value_mode` 决定，与缓存无关，故改 `value_mode` 不必让缓存失效。
//!
//! 覆盖值（用户手动改的）与自动取回的值共用一行，靠 `is_override` 区分。写覆盖值会覆盖
//! 上一次自动取回的值；覆盖值过期后需要重新取数。

use rusqlite::{params, Connection, Row};

use crate::domain::DynamicCacheEntry;
use crate::error::AppError;

/// 读一个（定义 × 环境）的缓存行。没有行不是错误——还没取过数是正常状态。
pub fn get(
    conn: &mut Connection,
    definition_id: &str,
    environment_id: &str,
) -> Result<Option<DynamicCacheEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT definition_id, environment_id, value_json, fetched_at, expires_at, \
                is_override, override_expires_at \
         FROM dynamic_variable_cache \
         WHERE definition_id = ?1 AND environment_id = ?2",
    )?;
    let mut rows = stmt.query(params![definition_id, environment_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_entry(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

/// 一条定义在所有环境键上仍然生效的覆盖值，最近的在前。
///
/// 数据源接口被删掉之后定义算不出环境（缓存键退化成空键），而覆盖值当初是按它那时生效的
/// 环境键写下的——故这时要横跨键去找，而不是只读空键那一行。只返回覆盖值：没有数据源支撑
/// 的自动值不该被这样翻出来。
///
/// 顺序定死：保存时刻最近的在前，同一时刻按环境键升序——SQLite 的自然行序不是契约。
pub fn live_overrides_for_definition(
    conn: &mut Connection,
    definition_id: &str,
    now: i64,
) -> Result<Vec<DynamicCacheEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT definition_id, environment_id, value_json, fetched_at, expires_at, \
                is_override, override_expires_at \
         FROM dynamic_variable_cache \
         WHERE definition_id = ?1 AND is_override = 1 AND value_json IS NOT NULL \
           AND (override_expires_at IS NULL OR override_expires_at > ?2) \
         ORDER BY fetched_at DESC, environment_id ASC",
    )?;
    let mut rows = stmt.query(params![definition_id, now])?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        found.push(row_to_entry(row).map_err(AppError::Sqlite)?);
    }
    Ok(found)
}

/// 写一行缓存。同一（定义 × 环境）已有行时整行覆盖。
pub fn upsert(conn: &mut Connection, entry: &DynamicCacheEntry) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO dynamic_variable_cache \
           (definition_id, environment_id, value_json, fetched_at, expires_at, \
            is_override, override_expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(definition_id, environment_id) DO UPDATE SET \
           value_json = ?3, fetched_at = ?4, expires_at = ?5, \
           is_override = ?6, override_expires_at = ?7",
        params![
            entry.definition_id,
            entry.environment_id,
            entry.value_json,
            entry.fetched_at,
            entry.expires_at,
            entry.is_override,
            entry.override_expires_at,
        ],
    )?;
    Ok(())
}

/// 删掉一个（定义 × 环境）的缓存行。「恢复自动获取」用它清掉覆盖值。
pub fn delete(
    conn: &mut Connection,
    definition_id: &str,
    environment_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM dynamic_variable_cache WHERE definition_id = ?1 AND environment_id = ?2",
        params![definition_id, environment_id],
    )?;
    Ok(())
}

/// 行映射器。
fn row_to_entry(row: &Row<'_>) -> Result<DynamicCacheEntry, rusqlite::Error> {
    Ok(DynamicCacheEntry {
        definition_id: row.get(0)?,
        environment_id: row.get(1)?,
        value_json: row.get(2)?,
        fetched_at: row.get(3)?,
        expires_at: row.get(4)?,
        is_override: row.get(5)?,
        override_expires_at: row.get(6)?,
    })
}

// 测试。

#[cfg(test)]
#[path = "dynamic_variable_cache_test.rs"]
mod tests;
