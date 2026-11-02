//! `environments` 表的 CRUD。
//!
//! 每个环境持有一张按模块的地址 map（`moduleBaseUrls`）和一个自由变量池。那张 map 是
//! **唯一**的 URL 层：单元格要么设要么没设，缺项解析为 `''`，不会回落到别的模块的值。
//!
//! 一个单元格（`ModuleBaseUrl`）有两个字段：`url` 是后端 api 地址，`proxy` 是实际访问
//! 地址——接口经代理网关转代后真正要打的那个地址。两者不一致时，请求发出前用 `proxy`
//! 替换 `url` 这个前缀。
//!
//! 这里**没有**「当前环境」这一列。当前环境是每个客户端自己的实时状态——桌面窗口、
//! 网页标签各一份，各切各的。落库会让 web 端多个人互相踩：一个人切了环境，另一个人
//! 发请求就换了地址与端口。
//!
//! 库里只有一个与「选哪个环境」有关的概念：**默认环境**。它是项目的属性，即一个项目里
//! **最早创建**的那个环境（按 `created_at` 认定，见 [`get_default`]）。谁发请求时没点名
//! 环境，就用它。
//!
//! 端点行也不再绑定环境（`endpoint_details` 没有 `environment_id` 列）：界面上「当前环境」
//! 说了算，MCP 侧则由每次调用自己点名，两条路都落到同一批数据上。

use std::collections::HashMap;

use rusqlite::{params, Connection, Row};

use crate::domain::{Environment, EnvironmentPatch, ModuleBaseUrl};
use crate::error::AppError;

/// 这里每次读共用的列清单，顺序同 [`row_to_env`]。
///
/// 用一个字符串而非几份拷贝：URL 列与变量列相邻，只点名其中一个的读会静默产出一个所有模块
/// 都解析成 `''` 的环境。
const COLUMNS: &str = "id, project_id, name, created_at, module_base_urls_json, variables_json, disabled_variables_json";

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
    let rows = stmt.query_map([], row_to_env)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 某项目的**默认环境**：一个项目里最早创建的那个环境。没有则 `None`。
///
/// 「最早创建」按 `created_at` 认定；同一毫秒里创建的两个环境按 id 决胜，故这个查询
/// 的结果唯一，与库里有多少个环境无关。
///
/// 这里曾经是 `get_active`（读 `is_active` 列）。那一列已随「当前环境」一起被清除：
/// 当前环境是客户端的实时状态，而默认环境是项目的属性，两者不是一回事。
pub fn get_default(
    conn: &mut Connection,
    project_id: &str,
) -> Result<Option<Environment>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM environments \
         WHERE project_id = ?1 \
         ORDER BY created_at ASC, id ASC \
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

/// 新建一个环境。返回**落库之后**的那一行。
///
/// 返回值带着**生效的** `created_at`：调用方通常不给它（结构体的默认值是 0），故
/// 这里补上当前时刻——存 0 的话这个环境会永远排在最前面，于是后来建的每一个环境
/// 都成不了默认环境。调用方必须回传这一份：把 0 回传出去，界面手里的创建时刻就与
/// 库里那一行对不上，于是「默认环境」在两侧算出两个答案。
pub fn create(conn: &mut Connection, e: &Environment) -> Result<Environment, AppError> {
    let module_base_urls_json = serde_json::to_string(&e.module_base_urls)?;
    let variables_json = serde_json::to_string(&e.variables)?;
    let disabled_variables_json = serde_json::to_string(&e.disabled_variables)?;
    // `created_at` 认定「默认环境」：一个项目里最早创建的那个。调用方通常不给它
    // （结构体的默认值是 0），故这里补上当前时刻——存 0 的话这个环境会永远排在最前面，
    // 于是后来建的每一个环境都成不了默认环境。
    let created_at = if e.created_at == 0 {
        crate::domain::now_ms()
    } else {
        e.created_at
    };
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, created_at, module_base_urls_json, variables_json, disabled_variables_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.id,
            e.project_id,
            e.name,
            created_at,
            module_base_urls_json,
            variables_json,
            disabled_variables_json,
        ],
    )?;
    // 读回这一行而不是就地拼：读回来的那份与 `GET /api/environments` 给的完全一致，
    // 调用方把它放进界面后不必再刷新一次。
    get(conn, &e.id)
}

pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
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

/// 删掉一个环境。
///
/// **必须在 `db::with_tx` 内调用**：删环境要与其他写一起提交。
///
/// 端点行不再绑定环境，故这里不需要解绑：删掉一个环境只是少了一个可选的环境，界面与
/// MCP 侧各自按「当前环境 / 点名的环境」回落，落到项目默认环境。
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
    let value = match env.variables.remove(from_key) {
        Some(v) => v,
        None => {
            return Err(AppError::Validation(format!(
                "rename_var: environment {env_id} has no variable {from_key:?}"
            )))
        }
    };
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

/// 写一个（环境 × 模块）格子的两个字段。
///
/// 两个参数各自「写什么就是什么」：
///
/// - `url` 传 `Some(u)` 写入后端 api 地址，传 `None` 则**这一格不动**；传空串表示清空它。
/// - `proxy` 传 `Some(p)` 写入实际访问地址，传 `None` 则**这一格不动**；传空串表示清空它。
///
/// 两个字段都空时整格删掉：一个既没有地址也没有代理的格子解析不出任何东西，留着只会
/// 让按键遍历的读法误判为「此处已配置」。
///
/// 此处不核对模块是否属于该环境的项目：那一对只有在 endpoint 真的去解析它时才成为 URL，
/// 而端点解析走的是端点所属项目的树。
/// **必须在 `db::with_tx` 内调用。** 返回更新后的 [`Environment`] 供渲染层回显。
pub fn set_module_base_url(
    conn: &mut Connection,
    env_id: &str,
    module_id: &str,
    url: Option<&str>,
    proxy: Option<&str>,
) -> Result<Environment, AppError> {
    // 与 `set_var` 同样的「载入 → 改 → 写」形状，同一个理由：对 JSON 列的一次原子 UPDATE 使
    // 读者看不到写了一半的 map。
    let mut env = get(conn, env_id)?;
    let cell = env.module_base_urls.get(module_id);
    let mut next = cell.cloned().unwrap_or_default();
    if let Some(u) = url {
        next.url = u.to_string();
    }
    if let Some(p) = proxy {
        next.proxy = if p.is_empty() {
            None
        } else {
            Some(p.to_string())
        };
    }
    if next.url.is_empty() && next.proxy.is_none() {
        env.module_base_urls.remove(module_id);
    } else {
        env.module_base_urls.insert(module_id.to_string(), next);
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
    let module_base_urls_json: String = row.get(4)?;
    let variables_json: String = row.get(5)?;
    let disabled_variables_json: String = row.get(6)?;
    let parse_err = |e: serde_json::Error, col: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(JsonColError(format!("{col}: {e}"))),
        )
    };
    let module_base_urls: HashMap<String, ModuleBaseUrl> =
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
        created_at: row.get(3)?,
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
