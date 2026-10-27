//! 行到结构体的映射：列表与单行读取共用同一组列。

use rusqlite::Row;

use crate::domain::{DynamicVariable, ValueMode};

/// 选出列表与单行读取共用的列。
pub(super) const COLUMNS: &str = "v.id, v.name, v.description, v.source_endpoint_id, n.name, \
                       v.source_inline_json, v.source_environment_id, \
                       v.source_base_url_definition_id, v.extract_path, v.value_mode, \
                       v.ttl_seconds, v.stale_fallback, v.enabled, v.sort_order, \
                       v.created_at, v.updated_at";

/// 行映射器。
pub(super) fn row_to_var(row: &Row<'_>) -> Result<DynamicVariable, rusqlite::Error> {
    let value_mode_raw: String = row.get(9)?;
    Ok(DynamicVariable {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        source_endpoint_id: row.get(3)?,
        source_endpoint_name: row.get(4)?,
        source_inline_json: row.get(5)?,
        source_environment_id: row.get(6)?,
        source_base_url_definition_id: row.get(7)?,
        extract_path: row.get(8)?,
        value_mode: ValueMode::parse(&value_mode_raw).unwrap_or_default(),
        ttl_seconds: row.get(10)?,
        stale_fallback: row.get(11)?,
        enabled: row.get(12)?,
        sort_order: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}
