//! 脚本链的装配与变量写回的合并规则。
//!
//! 顺序：**最外层 collection → 嵌套 collection → 端点**。pre-request 与 test 两个阶段
//! 同序——Postman 官方文档如此（"This run order applies to both pre-request and
//! post-response scripts"）。别按「test 反序」那类第三方说法改反。
//!
//! 装配只认「谁带了哪段脚本」，不认「谁在什么时候跑」——后者由 [`super::run`] 决定。

use std::collections::{BTreeMap, HashSet};

use rusqlite::params;

use super::types::{ScriptSource, ScriptUnit};
use crate::db;
use crate::domain::EnvWrite;
use crate::error::AppError;

/// 装配一个端点的脚本链。
///
/// 端点自己的脚本在 `endpoint_details` 里，祖先 collection 的在 `node_scripts` 里。
/// 没有配置脚本的节点不进链。
pub fn collect_chain(
    conn: &mut rusqlite::Connection,
    endpoint_node_id: &str,
) -> Result<Vec<ScriptUnit>, AppError> {
    let mut units: Vec<ScriptUnit> = Vec::new();

    // 沿 `parent_id` 上溯。收集到的是「端点 → 父 → 祖父」，故最后要反转。
    let mut chain: Vec<(String, String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut current: Option<String> = Some(endpoint_node_id.to_string());
    while let Some(id) = current {
        // `seen` 防环：树是用户可拖的，环不该让这里死循环。
        if !seen.insert(id.clone()) {
            break;
        }
        let row: Option<(String, Option<String>, String, String)> = conn
            .query_row(
                "SELECT id, parent_id, type, name FROM nodes WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();
        let Some((node_id, parent_id, kind, name)) = row else {
            break;
        };
        chain.push((node_id, kind, name));
        current = parent_id;
    }

    let endpoint_name = chain
        .first()
        .map(|(_, _, name)| name.clone())
        .unwrap_or_default();

    // 祖先里只有 collection 带脚本，模块没有。
    let mut ancestors: Vec<(String, String)> = chain
        .iter()
        .skip(1)
        .filter(|(_, kind, _)| kind == "collection")
        .map(|(id, _, name)| (id.clone(), name.clone()))
        .collect();
    ancestors.reverse();

    for (id, name) in ancestors {
        let scripts = db::node_scripts::get(conn, &id)?;
        if scripts.pre_request_script.is_none() && scripts.test_script.is_none() {
            continue;
        }
        units.push(ScriptUnit {
            source: ScriptSource::Collection { id, name },
            pre_request: scripts.pre_request_script,
            test: scripts.test_script,
        });
    }

    // 端点自己的脚本。没有 detail 行 = 这个端点没有配置，不算错误。
    let detail = match db::endpoint_details::get(conn, endpoint_node_id) {
        Ok(detail) => detail,
        Err(AppError::NotFound(_)) => return Ok(units),
        Err(e) => return Err(e),
    };
    if detail.pre_request_script.is_some() || detail.test_script.is_some() {
        units.push(ScriptUnit {
            source: ScriptSource::Endpoint {
                id: endpoint_node_id.to_string(),
                name: endpoint_name,
            },
            pre_request: detail.pre_request_script,
            test: detail.test_script,
        });
    }

    Ok(units)
}

/// 把脚本写下的变量并进环境的变量表。`value` 为 `None` 的写入表示删除该变量。
///
/// 合并规则只此一处：桌面端与 MCP 端都走它，两侧对「脚本写了变量」的解读才不会分叉。
pub fn merge_env(variables: &mut BTreeMap<String, String>, writes: &[EnvWrite]) {
    for write in writes {
        match &write.value {
            Some(value) => {
                variables.insert(write.key.clone(), value.clone());
            }
            None => {
                variables.remove(&write.key);
            }
        }
    }
}
