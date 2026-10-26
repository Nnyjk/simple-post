//! `dynamic_variables` 表的 CRUD —— 一条动态变量的定义。
//!
//! 定义是**全局**的：任何项目的端点都能引用它，整个工作区里变量名唯一。数据源二选一：
//! 引用树里的一个端点，或一份内联请求。
//!
//! 变量名的判据是 `^[A-Za-z0-9_]+$`——与 `{{var}}` 替换原语的 `\w+` 一致
//! （见 `crate::template::is_word`），否则声明的名字在替换时永远不生效。这个判据住在
//! 写路径上：两条 transport 都经过这里，故不会出现「一边能存一边不能存」。
//!
//! 数据源的当前值不在本表，在 `super::dynamic_variable_cache`。

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::{DynamicVariable, DynamicVariablePatch, ValueMode};
use crate::error::AppError;

/// 列出全部定义，按 `sort_order` 排序。
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

/// 按名字读一条定义。解析阶段用它把 `{{sns}}` 里的名字对上定义。
pub fn get_by_name(conn: &mut Connection, name: &str) -> Result<Option<DynamicVariable>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM dynamic_variables v \
         LEFT JOIN nodes n ON n.id = v.source_endpoint_id \
         WHERE v.name = ?1",
        COLUMNS
    ))?;
    let mut rows = stmt.query(params![name])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_var(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

/// 一条定义的数据源端点所属的项目。数据源是内联请求时没有项目。
///
/// MCP 工具用它决定「这次操作算在哪个项目的权限下」——定义本身是全局的，但取数会真的
/// 用到那个项目的 base URL 与鉴权。
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
    match super::nodes::root_id_of(conn, &endpoint_id) {
        Ok(root) => Ok(Some(root)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// 新建一条定义。
///
/// 变量名非法或与另一条定义重名时返回 `Validation`——重名会撞上
/// `idx_dynamic_vars_name` 那条唯一索引，与其把数据库的约束错误泄给用户，不如在这里
/// 说清是哪个名字撞了。
pub fn create(conn: &mut Connection, def: &DynamicVariable) -> Result<(), AppError> {
    if !is_valid_name(&def.name) {
        return Err(AppError::Validation(format!(
            "变量名 '{}' 非法：只能用字母、数字与下划线",
            def.name
        )));
    }
    // 数据源二选一，且**必须有一个**：没有数据源的定义永远解析不出值。
    //
    // 界面上「两个来源字段都为空」只有一个成因——引用的端点被删了（外键把它置成
    // NULL）。故这条校验也顺带保证了界面能区分「没有数据源」与「数据源已删除」。
    if def.source_endpoint_id.is_none() && def.source_inline_json.is_none() {
        return Err(AppError::Validation(
            "动态变量必须有一个数据源：引用一个接口，或给一份内联请求".to_string(),
        ));
    }
    if def.source_endpoint_id.is_some() && def.source_inline_json.is_some() {
        return Err(AppError::Validation(
            "动态变量的数据源只能是其中一种：引用接口，或内联请求".to_string(),
        ));
    }
    if let Some(existing) = get_by_name(conn, &def.name)? {
        if existing.id != def.id {
            return Err(AppError::Validation(format!(
                "已有一个叫 '{}' 的动态变量",
                def.name
            )));
        }
    }
    let (environment_id, base_url_definition_id) = check_source_bindings(
        conn,
        def.source_endpoint_id.as_deref(),
        def.source_environment_id.as_deref(),
        def.source_base_url_definition_id.as_deref(),
    )?;
    let result = conn.execute(
        "INSERT INTO dynamic_variables \
           (id, name, description, source_endpoint_id, source_inline_json, \
            source_environment_id, source_base_url_definition_id, extract_path, value_mode, \
            ttl_seconds, stale_fallback, enabled, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            def.id,
            def.name,
            def.description,
            def.source_endpoint_id,
            def.source_inline_json,
            environment_id,
            base_url_definition_id,
            def.extract_path,
            def.value_mode.as_str(),
            def.ttl_seconds,
            def.stale_fallback,
            def.enabled,
            def.sort_order,
            def.created_at,
            def.updated_at,
        ],
    );
    if let Err(e) = result {
        // 唯一索引兜底：并发写入时上面的检查可能刚被别人抢先。
        match unique_violation_columns(&e) {
            Some("name") => {
                return Err(AppError::Validation(format!(
                    "已有一个叫 '{}' 的动态变量",
                    def.name
                )));
            }
            Some("id") => {
                return Err(AppError::Validation(format!(
                    "动态变量的 id '{}' 已被占用",
                    def.id
                )));
            }
            Some(_) => {}
            None if is_unique_violation(&e) => {
                return Err(AppError::Validation(format!(
                    "已有一个叫 '{}' 的动态变量",
                    def.name
                )));
            }
            None => {}
        }
        return Err(AppError::Sqlite(e));
    }
    Ok(())
}

/// 改一条定义。
///
/// 改 `name` 时同样要过合法性检查与重名检查。
pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: DynamicVariablePatch,
) -> Result<DynamicVariable, AppError> {
    let current = get(conn, id)?;
    if let Some(name) = patch.name.as_ref() {
        if !is_valid_name(name) {
            return Err(AppError::Validation(format!(
                "变量名 '{name}' 非法：只能用字母、数字与下划线"
            )));
        }
        if name != &current.name {
            if let Some(existing) = get_by_name(conn, name)? {
                if existing.id != id {
                    return Err(AppError::Validation(format!(
                        "已有一个叫 '{name}' 的动态变量"
                    )));
                }
            }
        }
    }

    // 数据源仍须二选一，但**只在 patch 真的动了来源字段时**才校验。
    // 「两个来源字段都为空」是「数据源接口被删了」（外键把它置成 NULL），那是一种真实状态，
    // 故这种定义仍应允许改别的字段——否则改个名就会收到一句盖住真因的报错。
    let touches_source = patch.source_endpoint_id.is_some() || patch.source_inline_json.is_some();

    // patch 的三态：缺席 = 不动，`null` = 清除，有值 = 写入。
    let next_endpoint_id = match &patch.source_endpoint_id {
        None => current.source_endpoint_id.clone(),
        Some(value) => value.clone(),
    };
    let next_inline_json = match &patch.source_inline_json {
        None => current.source_inline_json.clone(),
        Some(value) => value.clone(),
    };
    if touches_source && next_endpoint_id.is_none() == next_inline_json.is_none() {
        return Err(AppError::Validation(
            "动态变量必须有一个数据源：引用一个接口，或给一份内联请求".to_string(),
        ));
    }

    let next_environment_id = match &patch.source_environment_id {
        None => current.source_environment_id.clone(),
        Some(value) => value.clone(),
    };
    let next_base_url_definition_id = match &patch.source_base_url_definition_id {
        None => current.source_base_url_definition_id.clone(),
        Some(value) => value.clone(),
    };
    let (environment_id, base_url_definition_id) = check_source_bindings(
        conn,
        next_endpoint_id.as_deref(),
        next_environment_id.as_deref(),
        next_base_url_definition_id.as_deref(),
    )?;

    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    let push = |sets: &mut Vec<String>,
                binds: &mut Vec<rusqlite::types::Value>,
                column: &str,
                value: rusqlite::types::Value| {
        sets.push(format!("{column} = ?{}", binds.len() + 1));
        binds.push(value);
    };

    if let Some(name) = patch.name {
        push(&mut sets, &mut binds, "name", name.into());
    }
    if let Some(description) = patch.description {
        push(
            &mut sets,
            &mut binds,
            "description",
            match description {
                Some(v) => v.into(),
                None => rusqlite::types::Value::Null,
            },
        );
    }
    if let Some(source_endpoint_id) = patch.source_endpoint_id {
        push(
            &mut sets,
            &mut binds,
            "source_endpoint_id",
            match source_endpoint_id {
                Some(v) => v.into(),
                None => rusqlite::types::Value::Null,
            },
        );
    }
    if let Some(source_inline_json) = patch.source_inline_json {
        push(
            &mut sets,
            &mut binds,
            "source_inline_json",
            match source_inline_json {
                Some(v) => v.into(),
                None => rusqlite::types::Value::Null,
            },
        );
    }
    // 环境与服务跟着数据源走：数据源换成内联请求时它们一并清空——内联请求自带完整
    // 地址，没有 base URL 可谈。
    push(
        &mut sets,
        &mut binds,
        "source_environment_id",
        match environment_id {
            Some(v) => v.into(),
            None => rusqlite::types::Value::Null,
        },
    );
    push(
        &mut sets,
        &mut binds,
        "source_base_url_definition_id",
        match base_url_definition_id {
            Some(v) => v.into(),
            None => rusqlite::types::Value::Null,
        },
    );
    if let Some(extract_path) = patch.extract_path {
        push(&mut sets, &mut binds, "extract_path", extract_path.into());
    }
    if let Some(value_mode) = patch.value_mode {
        push(
            &mut sets,
            &mut binds,
            "value_mode",
            value_mode.as_str().to_string().into(),
        );
    }
    if let Some(ttl_seconds) = patch.ttl_seconds {
        push(&mut sets, &mut binds, "ttl_seconds", ttl_seconds.into());
    }
    if let Some(stale_fallback) = patch.stale_fallback {
        push(
            &mut sets,
            &mut binds,
            "stale_fallback",
            stale_fallback.into(),
        );
    }
    if let Some(enabled) = patch.enabled {
        push(&mut sets, &mut binds, "enabled", enabled.into());
    }
    if let Some(sort_order) = patch.sort_order {
        push(&mut sets, &mut binds, "sort_order", sort_order.into());
    }

    if sets.is_empty() {
        return Ok(current);
    }
    push(
        &mut sets,
        &mut binds,
        "updated_at",
        crate::domain::now_ms().into(),
    );
    let sql = format!(
        "UPDATE dynamic_variables SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(id.to_string().into());
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("dynamic_variable {id}")));
    }
    get(conn, id)
}

/// 删一条定义。缓存行由外键 `ON DELETE CASCADE` 带走。
pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM dynamic_variables WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("dynamic_variable {id}")));
    }
    Ok(())
}

/// 一条动态变量被哪些端点引用。
#[derive(Debug, Clone)]
pub struct Reference {
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub project_id: String,
    pub project_name: String,
}

/// 哪些端点的请求里出现了 `{{name}}`。跨全部项目——定义是全局的，引用它的端点可能
/// 散在基线项目与各个分发项目里。
///
/// 粗筛用 SQL 的 `LIKE`（请求体、header、参数、鉴权四段拼起来查），细查用
/// `crate::template::list_missing_vars` 逐行确认——`LIKE` 会把 `{{name}}` 当普通子串命中，
/// 但也可能命中 `{{name}}` 之外的东西（例如值里恰好含这段文本），故粗筛之后还要细查。
pub fn referencing_endpoints(
    conn: &mut Connection,
    name: &str,
) -> Result<Vec<Reference>, AppError> {
    let needle = format!("%{{{{{name}}}}}%");
    let mut stmt = conn.prepare(
        "SELECT d.node_id, n.name, n.root_id, COALESCE(p.name, ''), \
                d.url, d.params_json, d.headers_json, d.body_json, d.auth_json \
         FROM endpoint_details d \
         JOIN nodes n ON n.id = d.node_id \
         LEFT JOIN projects p ON p.id = n.root_id \
         WHERE d.url LIKE ?1 ESCAPE '\\' \
            OR d.params_json LIKE ?1 ESCAPE '\\' \
            OR d.headers_json LIKE ?1 ESCAPE '\\' \
            OR d.body_json LIKE ?1 ESCAPE '\\' \
            OR d.auth_json LIKE ?1 ESCAPE '\\'",
    )?;
    let rows = stmt.query_map(params![needle], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;

    let mut out = Vec::new();
    for r in rows {
        let (
            node_id,
            node_name,
            project_id,
            project_name,
            url,
            params_json,
            headers_json,
            body_json,
            auth_json,
        ) = r.map_err(AppError::Sqlite)?;
        let mut texts: Vec<String> = vec![url];
        for json in [params_json, headers_json, body_json, auth_json] {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                collect_markers_from_json(&value, &mut texts);
            }
        }
        if texts.iter().any(|t| {
            crate::template::list_missing_vars(t)
                .iter()
                .any(|m| m == name)
        }) {
            out.push(Reference {
                endpoint_id: node_id,
                endpoint_name: node_name,
                project_id,
                project_name,
            });
        }
    }
    Ok(out)
}

/// 把一个 JSON 值里出现的所有字符串收集出来（键与值都要——值里的 `{{name}}` 也算数）。
fn collect_markers_from_json(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_markers_from_json(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_markers_from_json(v, out);
            }
        }
        _ => {}
    }
}

/// SQLite 的唯一约束冲突。
fn is_unique_violation(e: &rusqlite::Error) -> bool {
    match e {
        rusqlite::Error::SqliteFailure(inner, _) => {
            inner.code == rusqlite::ErrorCode::ConstraintViolation
        }
        _ => false,
    }
}

/// 冲突的是哪一条唯一约束：主键，还是「变量名」那条索引。
///
/// SQLite 只给出「UNIQUE constraint failed: <表>.<列>…」这样一条消息，故按列名分辨。
/// 分不出来时返回 `None`，调用方给一句不含糊的总错误。
fn unique_violation_columns(e: &rusqlite::Error) -> Option<&'static str> {
    let rusqlite::Error::SqliteFailure(inner, Some(message)) = e else {
        return None;
    };
    if inner.code != rusqlite::ErrorCode::ConstraintViolation {
        return None;
    }
    if message.contains("dynamic_variables.name") {
        Some("name")
    } else if message.contains("dynamic_variables.id") {
        Some("id")
    } else {
        None
    }
}

/// 校验取数用的环境与服务，并给出真正落库的两个 id。
///
/// 两者都必须属于数据源端点所属的项目：环境的格子按 (环境 × 服务 × 模块) 寻址，服务
/// 住在项目里，故拿别的项目的环境或服务去取数会取到别处的内容。
///
/// 数据源是内联请求时两者都清空——内联请求自带完整地址，没有 base URL 可谈。
fn check_source_bindings(
    conn: &mut Connection,
    endpoint_id: Option<&str>,
    environment_id: Option<&str>,
    base_url_definition_id: Option<&str>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let Some(endpoint_id) = endpoint_id else {
        return Ok((None, None));
    };
    let project_id = super::nodes::root_id_of(conn, endpoint_id)
        .map_err(|_| AppError::Validation(format!("数据源接口 {endpoint_id} 不存在")))?;
    if let Some(env_id) = environment_id {
        let owner = super::environments::lookup_project_id(conn, env_id)
            .map_err(|_| AppError::Validation(format!("环境 {env_id} 不存在")))?;
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "环境 {env_id} 不在数据源接口所属的项目里"
            )));
        }
    }
    if let Some(def_id) = base_url_definition_id {
        let owner = super::base_url_defs::project_id_of(conn, def_id)
            .map_err(|_| AppError::Validation(format!("服务 {def_id} 不存在")))?;
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "服务 {def_id} 不在数据源接口所属的项目里"
            )));
        }
    }
    Ok((
        environment_id.map(str::to_string),
        base_url_definition_id.map(str::to_string),
    ))
}

/// 变量名是否合法：只允许字母、数字与下划线，且非空。
///
/// 与 `crate::template::is_word` 同一套判据——`{{my-var}}` 这样的名字替换原语认不出来，
/// 存进去就等于存了一条永远不生效的定义。
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 选出列表与单行读取共用的列。
const COLUMNS: &str = "v.id, v.name, v.description, v.source_endpoint_id, n.name, \
                       v.source_inline_json, v.source_environment_id, \
                       v.source_base_url_definition_id, v.extract_path, v.value_mode, \
                       v.ttl_seconds, v.stale_fallback, v.enabled, v.sort_order, \
                       v.created_at, v.updated_at";

/// 行映射器。
fn row_to_var(row: &Row<'_>) -> Result<DynamicVariable, rusqlite::Error> {
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

// 测试。

#[cfg(test)]
#[path = "dynamic_variables_test.rs"]
mod tests;
