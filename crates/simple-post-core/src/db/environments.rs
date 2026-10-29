//! `environments` 表的 CRUD。
//!
//! 每个环境持有一张按 (服务 × 模块) 的 URL map（`moduleBaseUrls`）和一个自由变量池。那张
//! map 是**唯一**的 URL 层：单元格要么设要么没设，缺项解析为 `''`，不会回落到别的模块的
//! 值，也不会回落到已下线的环境级 map。`base_urls_json` 列仍在表里——删列 = 重建表 =
//! bump `SCHEMA_VERSION` = `ensure_current_schema` 删掉用户的库——但此处不读不写；唯一的
//! 读者是 `db::migrate` 里的那个迁移。
//!
//! `set_active` 是事务性的「降级其他、升级这个」——事务必须把写锁跨在两步上，否则两次并发
//! 的 set_active 会留下两个激活环境。这里不取锁，调用方须把 `set_active` 包进 `db::with_tx`。

use std::collections::HashMap;

use rusqlite::{params, Connection, Row};

use crate::domain::{Environment, EnvironmentPatch};
use crate::error::AppError;

/// 这里每次读共用的列清单，顺序同 [`row_to_env`]。
///
/// 用一个字符串而非四份拷贝：两个 URL 列彼此相邻，只点名其中一个的读会静默产出一个所有模块
/// 都解析成 `''` 的环境。
const COLUMNS: &str = "id, project_id, name, is_active, base_urls_json, module_base_urls_json, variables_json, disabled_variables_json";

// 读。

/// 一个项目下的全部环境，按 `name` 排序以让选择器显示稳定。
pub fn list_by_project(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Vec<Environment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE project_id = ?1 \
         ORDER BY name ASC, id ASC"
    ))?;
    let rows = stmt.query_map(params![project_id], row_to_env)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 跨全部项目的全部环境——供 REST `GET /api/environments` 未给 `projectId` 过滤时使用。
pub fn list_all(conn: &mut Connection) -> Result<Vec<Environment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         ORDER BY project_id ASC, name ASC, id ASC"
    ))?;
    let rows = stmt.query_map(params![], row_to_env)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 某项目唯一激活的环境，没有则 `None`。schema 不强制「只有一行激活」的不变式（那是 UI
/// 的职责），故这里返回 `is_active = 1` 的那一行；[`set_active`] 负责维持该不变式。
pub fn get_active(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Option<Environment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE project_id = ?1 AND is_active = 1 \
         LIMIT 1"
    ))?;
    let mut rows = stmt.query(params![project_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_env(row).map_err(AppError::Sqlite)?)),
        None => Ok(None),
    }
}

pub fn get(conn: &mut Connection, id: &str) -> Result<Environment, AppError> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM environments WHERE id = ?1"))?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_env(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("environment {id}"))),
    }
}

/// 查 `env_id` 所属的 `project_id`。env id 未知时返回 `NotFound`——调用方（会改动的 MCP
/// 工具）需要这个解析结果来施加逐项目权限检查；把未知 id 当作静默的「global」就是一个
/// 权限绕过。
///
/// 这正是补上那一环的工具：
///   tool_env_update(env_id="e_unknown") ⇒ lookup_project_id 失败
///   ⇒ NotFound ⇒ handler 在动任何东西之前就返回 NotFound。
pub fn lookup_project_id(conn: &mut Connection, env_id: &str) -> Result<String, AppError> {
    let pid: Option<String> = conn
        .query_row(
            "SELECT project_id FROM environments WHERE id = ?1",
            params![env_id],
            |r| r.get(0),
        )
        .ok();
    pid.ok_or_else(|| AppError::NotFound(format!("environment {env_id}")))
}

// 写。

pub fn create(conn: &mut Connection, e: &Environment) -> Result<(), AppError> {
    let module_base_urls_json = serde_json::to_string(&e.module_base_urls)?;
    let variables_json = serde_json::to_string(&e.variables)?;
    let disabled_variables_json = serde_json::to_string(&e.disabled_variables)?;
    // `base_urls_json` 刻意不在这个列表里。点名它会继续写一个无人读的列，而把调用方的遗留
    // `baseUrls` 带进去会留下一个在下一个读者眼里像是活的 blob。不写它就让 schema 默认值
    // 生效，那正是本构建写的每一行上 `base_urls_json` 取到的默认值。
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, is_active, module_base_urls_json, variables_json, disabled_variables_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.id,
            e.project_id,
            e.name,
            e.is_active as i64,
            module_base_urls_json,
            variables_json,
            disabled_variables_json,
        ],
    )?;
    Ok(())
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
    // 注意：`is_active` 在这里**也会**被 patch。`set_active` 才是负责维持「恰好一个激活」
    // 的那一处，但它清零其余行而这里不——两者都写 `is_active` 时，「恰好一个」就不再成立。
    // 这处分工尚未对齐，不要把本函数当作翻转激活状态的正规入口。
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = &patch.name {
        sets.push(format!("name = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(name.clone()));
    }
    if let Some(m) = &patch.module_base_urls {
        sets.push(format!("module_base_urls_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(m)?));
    }
    // `patch.base_urls` 收下但**忽略**——它背后的列已死（见模块文档）。写它会让一个已下线的
    // 层在下一个读者眼里像是现役的；而把该字段从 patch 类型里删掉，则会让旧客户端的载荷
    // 直接失败。故静默跳过是取舍。
    if let Some(v) = &patch.variables {
        sets.push(format!("variables_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(v)?));
    }
    if let Some(d) = &patch.disabled_variables {
        // 存成排序后的形状：集合的两种写法（逐个 toggle 与整份替换）落到同一份规范形状，
        // 读回来时不会因为调用方给的顺序不同而变。
        let mut sorted = d.clone();
        sorted.sort();
        sets.push(format!("disabled_variables_json = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(serde_json::to_string(
            &sorted,
        )?));
    }
    if let Some(a) = patch.is_active {
        sets.push(format!("is_active = ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Integer(a as i64));
    }

    if sets.is_empty() {
        return get(conn, id);
    }
    let sql = format!(
        "UPDATE environments SET {} WHERE id = ?{}",
        sets.join(", "),
        binds.len() + 1
    );
    binds.push(rusqlite::types::Value::Text(id.to_string()));
    let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    get(conn, id)
}

/// 把 `id` 升级为激活，并把同项目其余环境降级。**必须在 `db::with_tx` 内调用**——两条
/// `UPDATE` 必须一起拿到写锁，否则并发的 `set_active` 会留下两个激活环境。
/// 返回更新后的环境，好让调用方直接回显给渲染层。
pub fn set_active(conn: &mut Connection, id: &str) -> Result<Environment, AppError> {
    // set_active 先查项目，好给降级划定范围。必须在两条 UPDATE 之前，以便 id 不存在时直接短路。
    let project_id: Option<String> = conn
        .query_row(
            "SELECT project_id FROM environments WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    let project_id = match project_id {
        Some(p) => p,
        None => return Err(AppError::NotFound(format!("environment {id}"))),
    };
    conn.execute(
        "UPDATE environments SET is_active = 0 \
         WHERE project_id = ?1 AND id != ?2",
        params![project_id, id],
    )?;
    conn.execute(
        "UPDATE environments SET is_active = 1 WHERE id = ?1",
        params![id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM environments WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {id}")));
    }
    Ok(())
}

/// 设置或清除环境上的单个变量。`value = None` 整个移除该键（故
/// `set_var(env, "k", None)` 就是按键删除的路径）；`value = Some(v)` upsert `k → v`。
/// **必须在 `db::with_tx` 内调用。** 返回更新后的 [`Environment`]，调用方无需再读一次
/// 即可回显给渲染层。
pub fn set_var(
    conn: &mut Connection,
    env_id: &str,
    key: &str,
    value: Option<&str>,
) -> Result<Environment, AppError> {
    // 读一次，改内存 map，再写 JSON 列。对单个键来说比 JSON 扩展的 SQL 函数便宜，且与
    // `get` 保持完全相同的往返形状。
    let mut env = get(conn, env_id)?;
    match value {
        Some(v) => {
            env.variables.insert(key.to_string(), v.to_string());
        }
        None => {
            env.variables.remove(key);
            // set_var 把键从 variables 里移除时，「它被停用」这件事也就没了。
            env.disabled_variables.retain(|k| k != key);
        }
    }
    let variables_json = serde_json::to_string(&env.variables)?;
    let disabled_variables_json = serde_json::to_string(&env.disabled_variables)?;
    let rows = conn.execute(
        "UPDATE environments SET variables_json = ?1, disabled_variables_json = ?2 WHERE id = ?3",
        params![variables_json, disabled_variables_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

/// 停用或启用一个环境变量。**必须在 `db::with_tx` 内调用。**
///
/// 停用的变量**照样存、照样显示，但不参与替换**——`{{x}}` 留在原样并进
/// `missing_vars`，用户看得见「这个变量没被替换」，而不是静默被替掉。
///
/// 键不存在时返回 [`AppError::Validation`]：停用一个不存在的变量没有意义，而静默成功会让
/// 调用方以为它生效了。
pub fn set_var_disabled(
    conn: &mut Connection,
    env_id: &str,
    key: &str,
    disabled: bool,
) -> Result<Environment, AppError> {
    let mut env = get(conn, env_id)?;
    if !env.variables.contains_key(key) {
        return Err(AppError::Validation(format!(
            "set_var_disabled: environment {env_id} has no variable {key:?}"
        )));
    }
    env.disabled_variables.retain(|k| k != key);
    if disabled {
        env.disabled_variables.push(key.to_string());
        env.disabled_variables.sort();
    }
    let disabled_variables_json = serde_json::to_string(&env.disabled_variables)?;
    let rows = conn.execute(
        "UPDATE environments SET disabled_variables_json = ?1 WHERE id = ?2",
        params![disabled_variables_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

/// 一次性重命名一个环境变量：`from_key` 变成 `to_key`，值保留。
/// 这是对该 JSON 列的一次 `UPDATE`，故那个键永远不会被观察到不存在。
/// **必须在 `db::with_tx` 内调用。**
/// 错误：
///   - 环境不存在时 [`AppError::NotFound`]
///   - [`AppError::Validation`]：`from_key` 不存在（没东西可改）、`to_key` 已存在（拒绝覆盖
///     另一个变量），或 `from_key == to_key`（无操作的改名几乎总是 UI bug）
///
/// 返回更新后的 [`Environment`]，调用方回显权威行而非乐观猜测。
pub fn rename_var(
    conn: &mut Connection,
    env_id: &str,
    from_key: &str,
    to_key: &str,
) -> Result<Environment, AppError> {
    if from_key == to_key {
        return Err(AppError::Validation(format!(
            "rename_var: from_key and to_key are both {from_key:?}"
        )));
    }
    let mut env = get(conn, env_id)?;
    let value = env.variables.remove(from_key).ok_or_else(|| {
        AppError::Validation(format!(
            "rename_var: environment {env_id} has no variable {from_key:?}"
        ))
    })?;
    if env.variables.contains_key(to_key) {
        // 恢复刚移除的键，好让这次中止把 map 留在未改动状态。
        env.variables.insert(from_key.to_string(), value);
        return Err(AppError::Validation(format!(
            "rename_var: environment {env_id} already has a variable {to_key:?}"
        )));
    }
    env.variables.insert(to_key.to_string(), value);
    // rename 把「它被停用」这件事一起搬过去，否则改完名的变量会静默变成启用的。
    let was_disabled = env.disabled_variables.iter().any(|k| k == from_key);
    env.disabled_variables.retain(|k| k != from_key);
    if was_disabled {
        env.disabled_variables.push(to_key.to_string());
        env.disabled_variables.sort();
    }

    // 与 `set_var` 同一个单次 UPDATE 形状：`variables_json` 整列写入，是原子的，故改名
    // 没有中间状态。
    let variables_json = serde_json::to_string(&env.variables)?;
    let disabled_variables_json = serde_json::to_string(&env.disabled_variables)?;
    let rows = conn.execute(
        "UPDATE environments SET variables_json = ?1, disabled_variables_json = ?2 WHERE id = ?3",
        params![variables_json, disabled_variables_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

/// 设置或清除一个 `moduleBaseUrls[def_id][module_id]` 单元格。
///
/// `url = None` 移除该单元格，而这是真实答案而非一个洞：(服务 × 模块) 是唯一的一层，
/// 故被清空的单元格意味着此模块对此服务无地址，解析到此为止。它不会回落到别的模块的值，
/// 也不会回落到已下线的环境级 map——两个模块把同一个服务指向不同 host 是常态，不是该被统一处理的
/// 错误。
/// 此处不核对 endpoint 的模块是否属于该环境的项目：这一对只有在 endpoint 真的去解析它时才
/// 成为 URL，而拦住跨项目服务被指名的是 `base_url_definitions.project_id_of`。
/// **必须在 `db::with_tx` 内调用。** 返回更新后的 [`Environment`] 供渲染层回显。
pub fn set_module_base_url(
    conn: &mut Connection,
    env_id: &str,
    def_id: &str,
    module_id: &str,
    url: Option<&str>,
) -> Result<Environment, AppError> {
    // 与 `set_var` 同样的「载入 → 改 → 写」形状，同一个理由：对 JSON 列的一次原子 UPDATE 使
    // 读者看不到写了一半的 map。嵌套 map 只在确实要设值时才创建，故清掉某服务最后一个单元格
    // 不会在它后面留下一个空 `{}`。
    let mut env = get(conn, env_id)?;
    match url {
        Some(u) => {
            env.module_base_urls
                .entry(def_id.to_string())
                .or_default()
                .insert(module_id.to_string(), u.to_string());
        }
        None => {
            if let Some(per_module) = env.module_base_urls.get_mut(def_id) {
                per_module.remove(module_id);
                if per_module.is_empty() {
                    env.module_base_urls.remove(def_id);
                }
            }
        }
    }
    let module_base_urls_json = serde_json::to_string(&env.module_base_urls)?;
    let rows = conn.execute(
        "UPDATE environments SET module_base_urls_json = ?1 WHERE id = ?2",
        params![module_base_urls_json, env_id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("environment {env_id}")));
    }
    Ok(env)
}

// 行映射器。

fn row_to_env(row: &Row<'_>) -> Result<Environment, rusqlite::Error> {
    let base_urls_json: String = row.get(4)?;
    let module_base_urls_json: String = row.get(5)?;
    let variables_json: String = row.get(6)?;
    let disabled_variables_json: String = row.get(7)?;
    let parse_err = |e: serde_json::Error, col: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(JsonColError(format!("{col}: {e}"))),
        )
    };
    // `base_urls` 解析了但从不使用：该列只为存活而留，解析它是为了让仍会发来该字段的客户端在
    // 线格式上仍然诚实。故一个畸形的 blob 仍会让这次读失败，而不是被静默报成空 map。
    let base_urls: HashMap<String, String> =
        serde_json::from_str(&base_urls_json).map_err(|e| parse_err(e, "base_urls_json"))?;
    let module_base_urls: HashMap<String, HashMap<String, String>> =
        serde_json::from_str(&module_base_urls_json)
            .map_err(|e| parse_err(e, "module_base_urls_json"))?;
    let variables: HashMap<String, String> =
        serde_json::from_str(&variables_json).map_err(|e| parse_err(e, "variables_json"))?;
    let disabled_variables: Vec<String> = serde_json::from_str(&disabled_variables_json)
        .map_err(|e| parse_err(e, "disabled_variables_json"))?;
    Ok(Environment {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        is_active: row.get::<_, i64>(3)? != 0,
        base_urls,
        module_base_urls,
        variables,
        disabled_variables,
    })
}

/// 包住字符串的小 `StdError`。需要它是因为 rusqlite 的 `FromSqlConversionFailure` 载荷要求
/// `Box<dyn StdError + Send + Sync + 'static>`。
#[derive(Debug)]
struct JsonColError(String);

impl std::fmt::Display for JsonColError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonColError {}

// 测试。

#[cfg(test)]
#[path = "environments_test.rs"]
mod tests;
