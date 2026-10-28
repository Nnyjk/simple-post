//! 写路径：新建、改、删。

use rusqlite::{params, Connection};

use crate::domain::{DynamicVariable, DynamicVariablePatch};
use crate::error::AppError;

use super::read::{get, get_by_name_in_project};
use super::validation::{
    check_source_bindings, is_unique_violation, is_valid_name, unique_violation_columns,
};

/// 新建一条定义。
///
/// 变量名非法、与同项目里的另一条定义重名、或归属项目不存在时返回 `Validation`。
///
/// 重名会撞上 `idx_dynamic_vars_project_name` 那条唯一索引，与其把数据库的约束错误泄给
/// 用户，不如在这里说清是哪个名字撞了。
pub fn create(conn: &mut Connection, def: &DynamicVariable) -> Result<(), AppError> {
    if !is_valid_name(&def.name) {
        return Err(AppError::Validation(format!(
            "变量名 '{}' 非法：只能用字母、数字与下划线",
            def.name
        )));
    }
    // 归属项目必须存在：定义挂在项目里，指向一个不存在的项目等于挂在虚空里。
    if let Some(project_id) = def.project_id.as_deref() {
        check_project_exists(conn, project_id)?;
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
    if let Some(project_id) = def.project_id.as_deref() {
        if let Some(existing) = get_by_name_in_project(conn, project_id, &def.name)? {
            if existing.id != def.id {
                return Err(AppError::Validation(format!(
                    "项目里已有一个叫 '{}' 的动态变量",
                    def.name
                )));
            }
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
           (id, project_id, name, description, source_endpoint_id, source_inline_json, \
            source_environment_id, source_base_url_definition_id, extract_path, transform_script, \
            value_mode, ttl_seconds, stale_fallback, enabled, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            def.id,
            def.project_id,
            def.name,
            def.description,
            def.source_endpoint_id,
            def.source_inline_json,
            environment_id,
            base_url_definition_id,
            def.extract_path,
            def.transform_script,
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
        // 唯一索引再挡一次：并发写入时上面的检查可能刚被别人抢先。
        match unique_violation_columns(&e) {
            Some("name") => {
                return Err(AppError::Validation(format!(
                    "项目里已有一个叫 '{}' 的动态变量",
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
                    "项目里已有一个叫 '{}' 的动态变量",
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
/// 改 `name` 时同样要过合法性检查与重名检查；改归属项目时，新项目里也不能有同名
/// 的定义。
pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: DynamicVariablePatch,
) -> Result<DynamicVariable, AppError> {
    let current = get(conn, id)?;
    // 改完之后这条定义会落在哪个项目里。归属项目变了，重名检查就得在新项目里做。
    let next_project_id = match patch.project_id.as_deref() {
        None => current.project_id.clone(),
        Some(project_id) => {
            check_project_exists(conn, project_id)?;
            Some(project_id.to_string())
        }
    };
    if let Some(name) = patch.name.as_ref() {
        if !is_valid_name(name) {
            return Err(AppError::Validation(format!(
                "变量名 '{name}' 非法：只能用字母、数字与下划线"
            )));
        }
        if name != &current.name {
            if let Some(project_id) = next_project_id.as_deref() {
                if let Some(existing) = get_by_name_in_project(conn, project_id, name)? {
                    if existing.id != id {
                        return Err(AppError::Validation(format!(
                            "项目里已有一个叫 '{name}' 的动态变量"
                        )));
                    }
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

    if let Some(project_id) = patch.project_id {
        push(&mut sets, &mut binds, "project_id", project_id.into());
    }
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
    // 地址，不需要 base URL。
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
    if let Some(transform_script) = patch.transform_script {
        push(
            &mut sets,
            &mut binds,
            "transform_script",
            match transform_script {
                Some(v) => v.into(),
                None => rusqlite::types::Value::Null,
            },
        );
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

/// 归属项目必须存在。
fn check_project_exists(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE id = ?1",
        params![project_id],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Err(AppError::Validation(format!("项目 {project_id} 不存在")));
    }
    Ok(())
}
