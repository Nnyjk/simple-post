//! 移动：环检查、类型矩阵、兄弟重排与整棵子树的 `root_id` 级联。

use rusqlite::{params, Connection};

use crate::domain::{ResourceNode, ResourceNodeType};
use crate::error::AppError;

use super::read::{get, subtree_ids};

/// 把一个或多个节点移到 `new_parent_id` 下的新兄弟位置。
/// `node_ids` 即节点在目标处应出现的顺序，故多选拖拽保留用户的挑选顺序。父为 `None` 只对
/// 模块有意义——模块的父是项目而非节点。
/// **必须在 `db::with_tx` 内调用**——每个兄弟一次 UPDATE 来重编号，调用方要它们一起落地。
/// 事后必须成立的五件事，按序：
/// 1. 没有节点落在自己的子树里（环）
/// 2. 每个被移动节点的类型都被新父接受
/// 3. 新父的兄弟是连续的 `0..N`
/// 4. 整棵子树的 `root_id` 与新父一致
/// 5. 每个被移动行带上新的 `parent_id` 与新的槽位
pub fn move_many(
    conn: &mut Connection,
    node_ids: &[String],
    new_parent_id: Option<&str>,
    target_index: i32,
) -> Result<(), AppError> {
    if node_ids.is_empty() {
        return Ok(());
    }

    // 先把被移动者全部读出来。下面每一步都要它们的类型与当前归属，而只读一次能让各步检查
    // 彼此一致——即便期间有触发器或并发写入者在场。
    let mut movers = Vec::with_capacity(node_ids.len());
    for id in node_ids {
        movers.push(get(conn, id)?);
    }

    // 1. 环检查。collection 嵌在自己的后代之下，会让此处往下的每次树遍历都绕圈。
    let moved_subtrees = subtree_ids(conn, node_ids)?;
    let subtree_set: std::collections::HashSet<&String> = moved_subtrees.iter().collect();
    if let Some(parent) = new_parent_id {
        if subtree_set.contains(&parent.to_string()) {
            return Err(AppError::Validation(format!(
                "cannot move a node under its own descendant ({parent})"
            )));
        }
    }

    // 2. 类型矩阵。模块没有节点父，故 `None` 是它唯一合法的落点；其余需要一个类型接受它的父。
    let new_root_id = match new_parent_id {
        None => {
            for m in &movers {
                if m.kind != ResourceNodeType::Module {
                    return Err(AppError::Validation(format!(
                        "only a module can sit at the top of a project; \
                         node {} is a {}",
                        m.id,
                        m.kind.as_str()
                    )));
                }
            }
            movers[0].root_id.clone()
        }
        Some(parent) => {
            let parent_node = get(conn, parent)?;
            for m in &movers {
                if !m.kind.may_under(parent_node.kind) {
                    return Err(AppError::Validation(format!(
                        "a {} cannot sit under a {}",
                        m.kind.as_str(),
                        parent_node.kind.as_str()
                    )));
                }
            }
            parent_node.root_id
        }
    };

    // 3. 重排新父的子节点。按目标作用域限定，故这是一条统一查询。
    let siblings: Vec<String> = match new_parent_id {
        Some(p) => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![p], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![&new_root_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
    };

    let mut order: Vec<String> = siblings
        .into_iter()
        .filter(|id| !node_ids.contains(id))
        .collect();
    let mut idx = target_index.max(0) as usize;
    if idx > order.len() {
        idx = order.len();
    }
    for (offset, id) in node_ids.iter().enumerate() {
        order.insert(idx + offset, id.clone());
    }

    let mut update = conn.prepare("UPDATE nodes SET sort_order = ?1 WHERE id = ?2")?;
    for (i, id) in order.iter().enumerate() {
        update.execute(params![i as i64, id])?;
    }
    drop(update);

    // 4. 为每棵被移动子树重新推导 `root_id`。它是推导值，故跨边界必须触及每一个后代——只改
    //    指名的行会让子树仍归档在旧项目下、对 `list_by_root` 不可见。
    if movers.iter().any(|m| m.root_id != new_root_id) {
        let mut update_root = conn.prepare("UPDATE nodes SET root_id = ?1 WHERE id = ?2")?;
        for id in &moved_subtrees {
            update_root.execute(params![&new_root_id, id])?;
        }
    }

    // 5. 挂上被移动者。
    let mut attach = conn.prepare("UPDATE nodes SET parent_id = ?1 WHERE id = ?2")?;
    for id in node_ids {
        attach.execute(params![new_parent_id, id])?;
    }
    Ok(())
}

/// 把一个模块转到另一个项目，并落到指定的顶层槽位。
/// [`move_many`] 表达不了的那一种移动。以 `new_parent_id` 为 `None` 调用时它没有父行可读
/// root，于是保留模块当前的 `root_id`——对「重排我的模块」正确，对「把这个模块移到另一个
/// 项目」错误，而后者正是 MCP `module_move` 工具一向的意思。
/// 单独成函数而不是给 `move_many` 加参数，因为两者差别不止一个参数值：跨边界还要重编
/// **旧**项目的顶层兄弟，否则那个项目的 `sort_order` 会留下空档。
/// `target_index` 为 `None` 表示追加。**必须在 `db::with_tx` 内调用**——两个项目的排序必须
/// 一起落地。
pub fn move_to_root(
    conn: &mut Connection,
    id: &str,
    new_root_id: &str,
    target_index: Option<i32>,
) -> Result<ResourceNode, AppError> {
    let node = get(conn, id)?;
    if node.kind != ResourceNodeType::Module {
        return Err(AppError::Validation(format!(
            "only a module can move between projects; node {id} is a {}",
            node.kind.as_str()
        )));
    }
    // 否则一个打错的项目 id 会把子树归到没有任何项目行支撑的 root 下，此后永远没人能列出它。
    let dest_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![new_root_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !dest_exists {
        return Err(AppError::NotFound(format!("project {new_root_id}")));
    }
    if node.root_id == new_root_id && target_index.is_none() {
        return Ok(node);
    }

    let moved_subtrees = subtree_ids(conn, &[id.to_string()])?;

    // 补上离开的那个项目留下的空档，除非它同时就是目的地——那种情况下面的插入反正会重编号，
    // 先做压缩只会白白改动同一批行。离开中的模块要排除：此刻它仍归档在旧 root 下，把它算
    // 进去会让它腾出的槽位继续占着，什么也不上移。
    if node.root_id != new_root_id {
        renumber_top_level(conn, &node.root_id, Some(id))?;
    }

    // 把模块放进目的项目的顶层，并把该项目其余顶层围绕它上移。
    let mut order = top_level_ids(conn, new_root_id)?
        .into_iter()
        .filter(|existing| existing != id)
        .collect::<Vec<_>>();
    let idx = match target_index {
        Some(i) => (i.max(0) as usize).min(order.len()),
        None => order.len(),
    };
    order.insert(idx, id.to_string());
    write_order(conn, &order)?;

    // 为子树重新推导 `root_id`，理由同 `move_many`：只改指名的行，会让每个后代仍归档在旧
    // 项目下、对 `list_by_root` 不可见。
    let mut update_root = conn.prepare("UPDATE nodes SET root_id = ?1 WHERE id = ?2")?;
    for sid in &moved_subtrees {
        update_root.execute(params![new_root_id, sid])?;
    }
    drop(update_root);

    get(conn, id)
}

/// 一个项目顶层节点的 id，按显示序。
fn top_level_ids(conn: &mut Connection, root_id: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let ids = stmt
        .query_map(params![root_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 按给定顺序把 `0..N` 写进 `sort_order`。
fn write_order(conn: &mut Connection, ids: &[String]) -> Result<(), AppError> {
    let mut update = conn.prepare("UPDATE nodes SET sort_order = ?1 WHERE id = ?2")?;
    for (i, id) in ids.iter().enumerate() {
        update.execute(params![i as i64, id])?;
    }
    Ok(())
}

/// 某成员离开后，把该项目的顶层顺序重新压成连续的 `0..N`。
/// `exclude` 是离开的那个成员——运行本函数时它仍归档在这个 root 下，把它留着会让它腾出的
/// 槽位继续占着。
fn renumber_top_level(
    conn: &mut Connection,
    root_id: &str,
    exclude: Option<&str>,
) -> Result<(), AppError> {
    let ids = top_level_ids(conn, root_id)?
        .into_iter()
        .filter(|id| Some(id.as_str()) != exclude)
        .collect::<Vec<_>>();
    write_order(conn, &ids)
}

/// `id` 在它当前兄弟中的零基位置。
pub(super) fn sibling_index(
    conn: &mut Connection,
    node: &ResourceNode,
    id: &str,
) -> Result<i32, AppError> {
    let ids: Vec<String> = match &node.parent_id {
        Some(p) => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![p], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id FROM nodes WHERE root_id = ?1 AND parent_id IS NULL \
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![&node.root_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        }
    };
    Ok(ids.iter().position(|x| x == id).unwrap_or(0) as i32)
}
