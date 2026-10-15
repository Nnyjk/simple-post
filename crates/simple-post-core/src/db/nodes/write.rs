//! `nodes` 表的写路径：插入、部分更新、删除与 endpoint 复制。
//!
//! `parent_id` 的改动绕道 [`reparent`]，因为移动正是必须同时保住类型矩阵与子树 `root_id`
//! 的那个操作。

use rusqlite::{params, Connection};

use crate::domain::{EndpointDetail, ResourceNode, ResourceNodePatch, ResourceNodeType};
use crate::error::AppError;

use super::placement::{check_detail_agreement, validate_placement};
use super::read::get;
use super::tree::move_many;

/// 插入一个节点；类型需要时连同它的 `endpoint_details` 行。
/// 两半一起落：缺 detail 行的节点、或缺节点的 detail 行，都是没有读者走得动的树。调用方为
/// endpoint 提供 `detail`，其余必须传 `None`——[`check_detail_agreement`] 在插入前强制，
/// 故此错误以校验错误报出，而不是日后表现为一个半填充的 endpoint。
pub fn create(
    conn: &mut Connection,
    node: &ResourceNode,
    detail: Option<&EndpointDetail>,
) -> Result<ResourceNode, AppError> {
    check_detail_agreement(node, detail)?;
    validate_placement(conn, node.kind, node.parent_id.as_deref(), &node.root_id)?;

    conn.execute(
        "INSERT INTO nodes \
         (id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            node.id,
            node.root_id,
            node.parent_id,
            node.kind.as_str(),
            node.name,
            node.description,
            node.sort_order,
            node.created_at,
            node.updated_at,
        ],
    )?;
    if let Some(d) = detail {
        super::super::endpoint_details::create(conn, d)?;
    }
    Ok(node.clone())
}

/// 在同一父下复制一个 endpoint 及其配置。
/// 只限 endpoint：模块或 collection 的「副本」是个空壳，UI 也没有表达它的方式。
/// 复制子树是另一个功能，不是 duplicate。
/// **必须在 `db::with_tx` 内调用**——节点与它的 detail 行必须一起落地，否则副本打不开。
/// `new_id` 让调用方命名副本；`None` 在此生成一个，那正无状态调用方（MCP 工具）要的。
/// 桌面 UI 传自己的 id，因为它做的是**乐观**复制：在本调用返回之前就把行写进 store、激活
/// 标签页并把用户直接送进重命名。此处生成的 id 会来得太晚——标签页、选中项与进行中的
/// 重命名都会指向一行不存在的记录，工作区把标签页渲染成「接口已删除」。与 `nodes_create`
/// 同一契约：接受 SPA 给的 id。
pub fn duplicate(
    conn: &mut Connection,
    id: &str,
    new_id: Option<&str>,
) -> Result<ResourceNode, AppError> {
    let node = get(conn, id)?;
    if node.kind != ResourceNodeType::Endpoint {
        return Err(AppError::Validation(format!(
            "only an endpoint can be duplicated; node {id} is a {}",
            node.kind.as_str()
        )));
    }
    let detail = super::super::endpoint_details::get(conn, id)?;
    let now = crate::domain::now_ms();
    let copy = ResourceNode {
        id: new_id
            .map(str::to_string)
            .unwrap_or_else(crate::domain::uid),
        // 用「 (副本)」而非英文标记：这一行的名字是用户在中文界面里读到的东西，而 SPA 乐观
        // 写入时用的是同一个后缀——两者不一致会让副本看起来在创建后一刻自己改了名。
        name: format!("{} (副本)", node.name),
        // 紧跟原件之后，不是 `MAX + 1`：把副本排到队尾，会让人正看着的 endpoint 复制后
        // 从光标底下跑掉。
        sort_order: node.sort_order + 1,
        created_at: now,
        updated_at: now,
        ..node
    };
    let copied_detail = EndpointDetail {
        node_id: copy.id.clone(),
        ..detail
    };
    create(conn, &copy, Some(&copied_detail))?;
    get(conn, &copy.id)
}

/// 结构列的部分更新。
/// `parent_id` 绕道 [`reparent`] 而非直接写入，因为移动正是必须同时保住类型矩阵与子树
/// `root_id` 的那个操作——直接 UPDATE `parent_id` 两样都做不到。
pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: ResourceNodePatch,
) -> Result<ResourceNode, AppError> {
    // 单独取名，好与它调用的 `reparent` 函数区分开。
    let reparent_to = patch.parent_id.clone();

    // 逐列拼装而非从固定列表来，因为每个字段都各自可选，而 `description` 上的双层 Option
    // 还要区分「保持不动」与「清空」。
    let mut named: Vec<&str> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(n) = &patch.name {
        named.push("name");
        binds.push(rusqlite::types::Value::Text(n.clone()));
    }
    if let Some(d) = &patch.description {
        named.push("description");
        binds.push(match d {
            Some(v) => rusqlite::types::Value::Text(v.clone()),
            None => rusqlite::types::Value::Null,
        });
    }
    if let Some(s) = patch.sort_order {
        named.push("sort_order");
        binds.push(rusqlite::types::Value::Integer(s));
    }

    if !named.is_empty() {
        let assignments = named
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c} = ?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        binds.push(rusqlite::types::Value::Text(id.to_string()));
        let sql = format!("UPDATE nodes SET {assignments} WHERE id = ?{}", binds.len());
        let rows = conn.execute(&sql, rusqlite::params_from_iter(binds.iter()))?;
        if rows == 0 {
            return Err(AppError::NotFound(format!("node {id}")));
        }
    }

    match reparent_to {
        // 缺席：没人要求移动。
        None => {}
        Some(new_parent) => reparent(conn, id, new_parent.as_deref())?,
    }

    get(conn, id)
}

/// 把节点移到新父下，且不改变它在兄弟中的位置。
/// [`move_many`] 的薄封装，好让单节点移动与多选拖拽不会分叉——环检查、类型矩阵与
/// `root_id` 级联全在一处。
pub fn reparent(
    conn: &mut Connection,
    id: &str,
    new_parent_id: Option<&str>,
) -> Result<(), AppError> {
    let node = get(conn, id)?;
    // 保持节点当前的位置：读出它在旧兄弟中的序号，用到新集合上复用。
    let index = super::tree::sibling_index(conn, &node, id)?;
    move_many(
        conn,
        std::slice::from_ref(&id.to_string()),
        new_parent_id,
        index,
    )
}

/// 删除一个节点及其整棵子树。
/// `parent_id` 上的 `ON DELETE CASCADE` 负责拆除，`projects` 上的 `root_id` 级联负责删项目。
/// 本函数唯一多做的事是把不存在的 id 报出来，而不是静默成功。
pub fn delete(conn: &mut Connection, id: &str) -> Result<(), AppError> {
    let rows = conn.execute("DELETE FROM nodes WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("node {id}")));
    }
    Ok(())
}
