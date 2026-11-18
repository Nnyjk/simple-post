//! 校验：变量名的合法性、数据源绑定的归属、唯一约束冲突的判别。

use rusqlite::Connection;

use crate::error::AppError;

/// 数据源端点是否可绑，并给出它所属的项目。内联请求没有项目，故 `Ok(None)`。
///
/// 分开成两半是为了让「改一条已有定义」能只纠正环境而保留端点：环境是数据源的一层细化
/// （它决定 base URL 与变量），端点才是数据源本身。
pub(super) fn check_source_endpoint(
    conn: &mut Connection,
    endpoint_id: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(endpoint_id) = endpoint_id else {
        return Ok(None);
    };
    let project_id = match crate::db::nodes::root_id_of(conn, endpoint_id) {
        Ok(project_id) => project_id,
        // 只有「节点不存在」是配置错误；数据库出错照抛，别让它冒充「接口已删」。
        Err(AppError::NotFound(_)) => {
            return Err(AppError::Validation(format!(
                "数据源接口 {endpoint_id} 不存在"
            )))
        }
        Err(e) => return Err(e),
    };
    Ok(Some(project_id))
}

/// 校验取数用的环境，并给出真正落库的那个 id。
///
/// 环境必须属于数据源端点所属的项目（`endpoint_project`）：环境的格子按 (环境 × 模块)
/// 寻址，拿别的项目的环境去取数会取到别处的内容。数据源是内联请求时它清空——内联请求自带
/// 完整地址。
pub(super) fn check_source_environment(
    conn: &mut Connection,
    environment_id: Option<&str>,
    endpoint_project: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(project_id) = endpoint_project else {
        return Ok(None);
    };
    if let Some(env_id) = environment_id {
        let owner = match crate::db::environments::lookup_project_id(conn, env_id) {
            Ok(owner) => owner,
            // 只有「环境行不存在」是配置错误。数据库出错照抛：`update` 会把这条
            // `Validation` 读成「沿用的旧环境已失效」，进而在只改名的 patch 里把它清成 NULL。
            Err(AppError::NotFound(_)) => {
                return Err(AppError::Validation(format!("环境 {env_id} 不存在")))
            }
            Err(e) => return Err(e),
        };
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "环境 {env_id} 不在数据源接口所属的项目里"
            )));
        }
    }
    Ok(environment_id.map(str::to_string))
}

/// 两半一起校验。新建走这条：新填的一对来源就是用户此刻的意图，错一处就该报出来。
pub(super) fn check_source_bindings(
    conn: &mut Connection,
    endpoint_id: Option<&str>,
    environment_id: Option<&str>,
) -> Result<Option<String>, AppError> {
    let project_id = check_source_endpoint(conn, endpoint_id)?;
    check_source_environment(conn, environment_id, project_id.as_deref())
}

/// 变量名是否合法：只允许字母、数字与下划线，且非空。与 `crate::template::is_word` 同一套
/// 判据——`{{my-var}}` 这样的名字替换原语认不出来。
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
