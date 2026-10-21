//! 脚本链的装配与变量写回的合并规则。
//!
//! 顺序：**最外层 collection → 嵌套 collection → 端点**。pre-request 与 test 两个阶段
//! 同序——Postman 官方文档如此（"This run order applies to both pre-request and
//! post-response scripts"）。别按「test 反序」那类第三方说法改反。
//!
//! 装配只认「谁带了哪段脚本」，不认「谁在什么时候跑」——后者由 [`super::run`] 决定。

use std::collections::{BTreeMap, HashSet};

use rusqlite::params;

use super::send::ScriptOverride;
use super::types::{ScriptSource, ScriptUnit};
use crate::db;
use crate::domain::EnvWrite;
use crate::error::AppError;

/// 装配一个端点的脚本链。
///
/// 端点自己的脚本在 `endpoint_details` 里，祖先 collection 的在 `node_scripts` 里。
/// 没有配置脚本的节点不进链。
///
/// `endpoint_override` 覆盖**端点自己**那两段脚本：`None` = 用存的那份；`Some` = 用这份
/// （空串 = 没有脚本）。渲染层送的是界面上此刻生效的那份，故未保存的编辑也会跑到；
/// 祖先 collection 的脚本仍从库里读——它们不在端点编辑器里改。
pub fn collect_chain(
    conn: &mut rusqlite::Connection,
    endpoint_node_id: &str,
    endpoint_override: Option<&ScriptOverride>,
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

    // 端点自己的脚本。覆盖优先，且**逐字段**覆盖：payload 里没提到的那个字段仍用存的那份。
    //
    // 没有 detail 行不算错误——草稿接口就没落库这一行；此时存的那份不存在。
    let (saved_pre, saved_test) = match db::endpoint_details::get(conn, endpoint_node_id) {
        Ok(detail) => (detail.pre_request_script, detail.test_script),
        Err(AppError::NotFound(_)) => (None, None),
        Err(e) => return Err(e),
    };
    let (pre_request, test) = match endpoint_override {
        Some(override_) => (
            override_.pre_request.clone().or(saved_pre),
            override_.test.clone().or(saved_test),
        ),
        None => (saved_pre, saved_test),
    };
    if pre_request.is_some() || test.is_some() {
        units.push(ScriptUnit {
            source: ScriptSource::Endpoint {
                id: endpoint_node_id.to_string(),
                name: endpoint_name,
            },
            pre_request,
            test,
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
