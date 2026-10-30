//! 校验：变量名的合法性、数据源绑定的归属、唯一约束冲突的判别。

use rusqlite::Connection;

use crate::error::AppError;

/// 校验取数用的环境，并给出真正落库的那个 id。
///
/// 环境必须属于数据源端点所属的项目：环境的格子按 (环境 × 模块) 寻址，拿别的项目的
/// 环境去取数会取到别处的内容。
///
/// 数据源是内联请求时它清空——内联请求自带完整地址，没有 base URL 可谈。
pub(super) fn check_source_bindings(
    conn: &mut Connection,
    endpoint_id: Option<&str>,
    environment_id: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(endpoint_id) = endpoint_id else {
        return Ok(None);
    };
    let project_id = crate::db::nodes::root_id_of(conn, endpoint_id)
        .map_err(|_| AppError::Validation(format!("数据源接口 {endpoint_id} 不存在")))?;
    if let Some(env_id) = environment_id {
        let owner = crate::db::environments::lookup_project_id(conn, env_id)
            .map_err(|_| AppError::Validation(format!("环境 {env_id} 不存在")))?;
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "环境 {env_id} 不在数据源接口所属的项目里"
            )));
        }
    }
    Ok(environment_id.map(str::to_string))
}

/// 变量名是否合法：只允许字母、数字与下划线，且非空。
///
/// 与 `crate::template::is_word` 同一套判据——`{{my-var}}` 这样的名字替换原语认不出来，
/// 存进去就等于存了一条永远不生效的定义。
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// SQLite 的唯一约束冲突。
pub(super) fn is_unique_violation(e: &rusqlite::Error) -> bool {
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
pub(super) fn unique_violation_columns(e: &rusqlite::Error) -> Option<&'static str> {
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
