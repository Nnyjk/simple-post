//! `endpoint_details` 表的 CRUD——资源节点的 endpoint 专属那一半。
//!
//! 从 `nodes` 拆出，因为 `method` / `url` / `body` / `auth` / `tags` 对模块或 collection
//! 毫无意义。折叠进去会让每个模块和 collection 行带上一组永远为空的列，也会让搜索里的
//! `method = ?` 过滤条件无物可分。
//!
//! `node_id` 既是主键也是回到 `nodes` 的连接键。树的列（name、sort_order、时间戳）刻意
//! 不出现在这里，好让它们各自只有一个归属——见 [`crate::domain::ResourceNode`]。
//!
//! 每个函数都取 `&mut Connection`，由调用方决定这次调用是否在事务内。同时触及两张表的
//! 写入走 [`super::nodes::create`] / [`super::nodes::delete`]，故两半不会走散。

use rusqlite::{params, Connection, Row};

use crate::domain::{AuthConfig, EndpointDetail, EndpointDetailPatch, KeyValue, RequestBody};
use crate::error::AppError;

use super::json_col_err;

const COLUMNS: &str = "node_id, method, url, environment_id, base_url_definition_id, \
                       base_url_override, params_json, headers_json, body_json, auth_json, \
                       docs, notes, tags_json, pre_request_script, test_script";

// 读。

/// 一个 endpoint 的配置。节点存在却没有 detail 行时为 `NotFound`——那意味着 `nodes` 与
/// `endpoint_details` 两张表不一致，值得暴露出来而不是给它一个默认值。
pub fn get(conn: &mut Connection, node_id: &str) -> Result<EndpointDetail, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM endpoint_details WHERE node_id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![node_id])?;
    match rows.next()? {
        Some(row) => row_to_detail(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!(
            "endpoint detail for node {node_id}"
        ))),
    }
}

/// 一次取多个节点的配置——树已经收窄到 `type = 'endpoint'`、想在同一次往返里连 body 一起
/// 拿时的形状。
pub fn list_by_node_ids(
    conn: &mut Connection,
    node_ids: &[String],
) -> Result<Vec<EndpointDetail>, AppError> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", node_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT {COLUMNS} FROM endpoint_details WHERE node_id IN ({placeholders}) \
         ORDER BY node_id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(node_ids.iter()), row_to_detail)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 一个项目下全部 endpoint 的配置，经 `nodes` join，好让 `root_id` 索引来收窄。
pub fn list_by_root(conn: &mut Connection, root_id: &str) -> Result<Vec<EndpointDetail>, AppError> {
    // 列清单在此写出而非复用 COLUMNS：这些列要用 `e` 别名限定，`nodes` 才能 join 进来。
    let sql = "SELECT e.node_id, e.method, e.url, e.environment_id, e.base_url_definition_id, \
                  e.base_url_override, e.params_json, e.headers_json, e.body_json, e.auth_json, \
                  e.docs, e.notes, e.tags_json, e.pre_request_script, e.test_script \
         FROM endpoint_details e \
         JOIN nodes n ON n.id = e.node_id \
         WHERE n.root_id = ?1 AND n.type = 'endpoint' \
         ORDER BY n.sort_order ASC, e.node_id ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![root_id], row_to_detail)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

// 写。

pub fn create(conn: &mut Connection, d: &EndpointDetail) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO endpoint_details \
         (node_id, method, url, environment_id, base_url_definition_id, \
          base_url_override, params_json, headers_json, body_json, auth_json, \
          docs, notes, tags_json, pre_request_script, test_script) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            d.node_id,
            d.method.as_str(),
            d.url,
            d.environment_id,
            d.base_url_definition_id,
            d.base_url_override,
            serde_json::to_string(&d.params)?,
            serde_json::to_string(&d.headers)?,
            serde_json::to_string(&d.body)?,
            serde_json::to_string(&d.auth)?,
            d.docs,
            d.notes,
            serde_json::to_string(&d.tags)?,
            d.pre_request_script,
            d.test_script,
        ],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    node_id: &str,
    patch: EndpointDetailPatch,
) -> Result<EndpointDetail, AppError> {
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(m) = &patch.method {
        sets.push(format!("method = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(m.as_str().to_string()));
    }
    if let Some(u) = &patch.url {
        sets.push(format!("url = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(u.clone()));
    }
    push_optional(
        &mut sets,
        &mut binds,
        "environment_id",
        &patch.environment_id,
    );
    push_optional(
        &mut sets,
        &mut binds,
        "base_url_definition_id",
        &patch.base_url_definition_id,
    );
    push_optional(
        &mut sets,
        &mut binds,
        "base_url_override",
        &patch.base_url_override,
    );
    if let Some(p) = &patch.params {
        sets.push(format!("params_json = ?{}", binds.len() + 1));
        binds.push(json(&p)?);
    }
    if let Some(h) = &patch.headers {
        sets.push(format!("headers_json = ?{}", binds.len() + 1));
        binds.push(json(&h)?);
    }
    if let Some(b) = &patch.body {
        sets.push(format!("body_json = ?{}", binds.len() + 1));
        binds.push(json(&b)?);
    }
    if let Some(a) = &patch.auth {
        sets.push(format!("auth_json = ?{}", binds.len() + 1));
        binds.push(json(&a)?);
    }
    push_optional(&mut sets, &mut binds, "docs", &patch.docs);
    push_optional(&mut sets, &mut binds, "notes", &patch.notes);
    if let Some(t) = &patch.tags {
        sets.push(format!("tags_json = ?{}", binds.len() + 1));
        binds.push(json(&t)?);
    }
    if let Some(s) = &patch.pre_request_script {
        sets.push(format!("pre_request_script = ?{}", binds.len() + 1));
        binds.push(optional_text(s));
    }
    if let Some(s) = &patch.test_script {
        sets.push(format!("test_script = ?{}", binds.len() + 1));
        binds.push(optional_text(s));
    }

    if sets.is_empty() {
        return get(conn, node_id);
    }
    let sql = format!(
        "UPDATE endpoint_details SET {} WHERE node_id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(node_id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!(
            "endpoint detail for node {node_id}"
        )));
    }
    get(conn, node_id)
}

// 内部实现。

fn json<T: serde::Serialize>(v: &T) -> Result<rusqlite::types::Value, AppError> {
    Ok(rusqlite::types::Value::Text(serde_json::to_string(v)?))
}

/// 为一个 `Option<Option<T>>` 字段追加 `col = ?n` 子句。
/// 双层 Option 是全领域统一的约定：缺席则该列不动，显式 null 则清空。
fn push_optional(
    sets: &mut Vec<String>,
    binds: &mut Vec<rusqlite::types::Value>,
    col: &str,
    value: &Option<Option<String>>,
) {
    if let Some(v) = value {
        sets.push(format!("{col} = ?{}", binds.len() + 1));
        binds.push(match v {
            Some(s) => rusqlite::types::Value::Text(s.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
}

/// 脚本列的值：清空写 NULL，保留写文本。
fn optional_text(value: &Option<String>) -> rusqlite::types::Value {
    match value {
        Some(s) => rusqlite::types::Value::Text(s.clone()),
        None => rusqlite::types::Value::Null,
    }
}

fn row_to_detail(row: &Row<'_>) -> Result<EndpointDetail, rusqlite::Error> {
    let method_str: String = row.get(1)?;
    let params_json: String = row.get(6)?;
    let headers_json: String = row.get(7)?;
    let body_json: String = row.get(8)?;
    let auth_json: String = row.get(9)?;
    let tags_json: String = row.get(12)?;

    // 一个存下来的动词若被解析器拒绝，本会是一个 `AppError::Validation` 从一个欠调用方一个
    // `rusqlite::Error` 的行映射器里逃出去；重新包一层，好让失败指明列名。
    let method = crate::domain::parse_http_method(&method_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(super::JsonColError(format!("method: {e}"))),
        )
    })?;

    Ok(EndpointDetail {
        node_id: row.get(0)?,
        method,
        url: row.get(2)?,
        environment_id: row.get(3)?,
        base_url_definition_id: row.get(4)?,
        base_url_override: row.get(5)?,
        params: serde_json::from_str::<Vec<KeyValue>>(&params_json)
            .map_err(|e| json_col_err("params_json", e))?,
        headers: serde_json::from_str::<Vec<KeyValue>>(&headers_json)
            .map_err(|e| json_col_err("headers_json", e))?,
        body: serde_json::from_str::<RequestBody>(&body_json)
            .map_err(|e| json_col_err("body_json", e))?,
        auth: serde_json::from_str::<AuthConfig>(&auth_json)
            .map_err(|e| json_col_err("auth_json", e))?,
        docs: row.get(10)?,
        notes: row.get(11)?,
        tags: serde_json::from_str::<Vec<String>>(&tags_json)
            .map_err(|e| json_col_err("tags_json", e))?,
        pre_request_script: row.get(13)?,
        test_script: row.get(14)?,
    })
}

// 测试。

#[cfg(test)]
#[path = "endpoint_details_test.rs"]
mod tests;
