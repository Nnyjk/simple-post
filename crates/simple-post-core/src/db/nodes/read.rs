//! `nodes` 表的读路径：列表、单行查找、子树与向上溯源。
//!
//! 父子矩阵住在 [`ResourceNodeType::may_under`]，写路径每次写入时执行；`schema.sql`
//! 另有配套触发器作为第二道防线。

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, Row};

use crate::domain::{ResourceNode, ResourceNodeType};
use crate::error::AppError;

const COLUMNS: &str = "id, root_id, parent_id, type, name, description, sort_order, \
                       created_at, updated_at";

/// 同一份列表，为要 join 第二张表的查询加上表限定。
const COLUMNS_N: &str = "n.id, n.root_id, n.parent_id, n.type, n.name, n.description, \
                         n.sort_order, n.created_at, n.updated_at";

/// 一个项目下的全部节点，按显示序排列。
/// 只取该项目，而不像无过滤的整库读取那样把整个数据库都拖回来。
pub fn list_by_root(conn: &mut Connection, root_id: &str) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM nodes WHERE root_id = ?1 \
         ORDER BY sort_order ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 一个项目下某种类型的全部节点。
/// `resource_type` 是插值而非绑定，因为 [`ResourceNodeType::as_str`] 从三个字面量的封闭集合
/// 返回——没有用户可控的字符串进入 SQL。
pub fn list_by_root_and_type(
    conn: &mut Connection,
    root_id: &str,
    resource_type: ResourceNodeType,
) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM nodes WHERE root_id = ?1 AND type = ?2 \
         ORDER BY sort_order ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![root_id, resource_type.as_str()], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 全库节点。供 MCP 的 list 工具使用：它们应答的是调用方指名的项目，而非 UI 当前激活的那个。
pub fn list_all(conn: &mut Connection) -> Result<Vec<ResourceNode>, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM nodes ORDER BY root_id ASC, sort_order ASC, id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

/// 对树做子串搜索。
/// endpoint 的 `url` 在 `endpoint_details`，故此处得 join 才能触及——配置被拆出去的同一个
/// 理由。模块与 collection 没有 url，`n.type = 'endpoint'` 这道守卫免得匹配上 null 列。
/// 只想只读的话 `with_conn` 足够；调用方若还要读命中 endpoint 的详情，则必须在
/// `db::with_tx` 内调用——join 本身是只读的。
pub fn search(conn: &mut Connection, q: &str) -> Result<Vec<ResourceNode>, AppError> {
    let pattern = format!("%{q}%");
    let sql = format!(
        "SELECT {COLUMNS_N} FROM nodes n \
         LEFT JOIN endpoint_details e ON e.node_id = n.id \
         WHERE n.name LIKE ?1 OR n.description LIKE ?1 \
            OR (n.type = 'endpoint' AND e.url LIKE ?1) \
         ORDER BY n.root_id ASC, n.sort_order ASC, n.id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![pattern], row_to_node)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(AppError::Sqlite)?);
    }
    Ok(out)
}

pub fn get(conn: &mut Connection, id: &str) -> Result<ResourceNode, AppError> {
    let sql = format!("SELECT {COLUMNS} FROM nodes WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => row_to_node(row).map_err(AppError::Sqlite),
        None => Err(AppError::NotFound(format!("node {id}"))),
    }
}

/// 一个节点所属的项目，一次查完。
/// `db::permissions` 里逐项目权限门的第一步，故每个会改动的 MCP 工具都会跑到它。
pub fn root_id_of(conn: &mut Connection, id: &str) -> Result<String, AppError> {
    conn.query_row(
        "SELECT root_id FROM nodes WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!("node {id}")),
        other => AppError::Sqlite(other),
    })
}

/// 某节点挂在哪个模块下，不在模块下则为 `None`。
/// 除模块本身外每个节点上方恰有一个模块，而 endpoint 配置的 base URL 正以该模块为键：
/// 现役值层是 `moduleBaseUrls[module]`，故解析一个 endpoint 的 URL 要先问这个节点属于哪个
/// 模块，再去那一格里取值。此函数是第一步。这是树上的问题而非该行自身的属性，故不像
/// `root_id` 那样反范式化。
/// `None` 是真实答案而非失败：该节点自身可能就是模块，或者根本不存在。需要区分二者的调用方
/// 先问 [`type_of`]，或干脆把「没有模块」当作「没有 base URL」，解析器正是这么做的。
/// 向上遍历跳过节点自身，故模块不会被当成自己的答案。
pub fn module_id_of(conn: &mut Connection, id: &str) -> Result<Option<String>, AppError> {
    let found: Option<String> = conn
        .query_row(
            "WITH RECURSIVE up(id, parent_id, type, depth) AS ( \
                 SELECT id, parent_id, type, 0 FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT p.id, p.parent_id, p.type, up.depth + 1 \
                 FROM nodes p JOIN up ON p.id = up.parent_id \
             ) \
             SELECT id FROM up WHERE type = 'module' AND depth > 0 LIMIT 1",
            params![id],
            |r| r.get(0),
        )
        .ok();
    Ok(found)
}

/// 节点的类型，或 `NotFound`。
pub fn type_of(conn: &mut Connection, id: &str) -> Result<ResourceNodeType, AppError> {
    let raw: String = conn
        .query_row("SELECT type FROM nodes WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(format!("node {id}")),
            other => AppError::Sqlite(other),
        })?;
    ResourceNodeType::parse(&raw).ok_or_else(|| {
        // CHECK 约束应当使这不可达；如实报出，好过把未知类型映射到某个随便的变体上。
        AppError::Db(format!("node {id} has unrecognised type {raw:?}"))
    })
}

/// `roots` 加上每个 root 的全部后代，BFS 序稳定。
/// 用 `UNION` 而非 `UNION ALL`，使数据中已存在的环终止遍历而不是永远绕圈。本函数之上
/// 没有人在写入时防环，故这里必须是那个能防的地方。
pub fn subtree_ids(conn: &mut Connection, roots: &[String]) -> Result<Vec<String>, AppError> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = String::from("WITH RECURSIVE sub(id) AS (");
    for (i, _) in roots.iter().enumerate() {
        if i > 0 {
            sql.push_str(" UNION ");
        }
        sql.push_str(&format!("SELECT id FROM nodes WHERE id = ?{}", i + 1));
    }
    sql.push_str(
        " UNION SELECT c.id FROM nodes c \
         JOIN sub ON c.parent_id = sub.id) \
         SELECT id FROM sub",
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(roots.iter()), |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 某个容器节点子树下的全部端点，按**树的显示顺序**排。
///
/// 顺序与界面渲染的那一套一致（渲染侧见 `src/features/project-tree/tree-utils.ts` 的
/// `buildTree`，它把一个容器自己的接口画在它的子集合**之上**）：
///
///   1. 容器**直接持有**的接口，按 `(sort_order, id)`；
///   2. 然后容器**直接持有**的子集合，按 `(sort_order, id)`，逐个递归——子集合自己的
///      接口先出，然后才是它的子集合。
///
/// 故这不是纯粹的深度优先前序：一个集合的接口全部出完，才轮到它的第一个子集合。
/// 批量运行按这个次序发请求，与用户在树上看到的行序一致。
///
/// `root_id` 指向的节点不存在时返回空列表（不是错误）——调用方通常已经查过那棵树，
/// 这里只回答「它下面有哪些端点」。
pub fn ordered_subtree_endpoints(
    conn: &mut Connection,
    root_id: &str,
) -> Result<Vec<ResourceNode>, AppError> {
    // 递归 CTE 取回整棵子树（含根自己）。用 `UNION` 而非 `UNION ALL`：数据里若已存在
    // 环，`UNION` 会终止遍历，而 `UNION ALL` 会永远绕圈——与 `subtree_ids` 同一处置。
    //
    // 排序在 Rust 侧做，不在 SQL 里：`sort_order` 是 `INTEGER NOT NULL` 而没有非负约束，
    // 把它拼成字符串路径再比大小，负数会排到正数前面去。
    let sql = "WITH RECURSIVE sub(id, root_id, parent_id, type, name, description, sort_order, \
                      created_at, updated_at) AS ( \
                   SELECT id, root_id, parent_id, type, name, description, sort_order, \
                          created_at, updated_at \
                   FROM nodes WHERE id = ?1 \
                   UNION \
                   SELECT n.id, n.root_id, n.parent_id, n.type, n.name, n.description, \
                          n.sort_order, n.created_at, n.updated_at \
                   FROM nodes n JOIN sub ON n.parent_id = sub.id \
               ) \
               SELECT id, root_id, parent_id, type, name, description, sort_order, \
                      created_at, updated_at FROM sub";
    let mut stmt = conn.prepare(sql)?;
    let nodes = stmt
        .query_map(params![root_id], row_to_node)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    // 父 → 子。同一父下的兄弟按 `(sort_order, id)` 排，与 `list_by_root` 与
    // `list_children` 的 `ORDER BY sort_order ASC, id ASC` 一致。
    let mut children: HashMap<String, Vec<ResourceNode>> = HashMap::new();
    let mut root_node: Option<ResourceNode> = None;
    for node in nodes {
        if node.id == root_id {
            root_node = Some(node.clone());
        }
        if let Some(parent) = node.parent_id.clone() {
            children.entry(parent).or_default().push(node);
        }
    }
    for bucket in children.values_mut() {
        bucket.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    // 根自己是端点时没有子树可言，直接回答它自己。
    let Some(root) = root_node else {
        return Ok(Vec::new());
    };
    if root.kind == ResourceNodeType::Endpoint {
        return Ok(vec![root]);
    }

    let mut out: Vec<ResourceNode> = Vec::new();
    // 栈里放「待展开的容器」，第一个就是根自己——根自己直接持有的接口也要出。
    // `seen` 防环：父子关系由用户拖拽决定，数据里出现环时这里不能绕圈。
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<ResourceNode> = vec![root];
    while let Some(container) = stack.pop() {
        if !seen.insert(container.id.clone()) {
            continue;
        }
        // 本容器直接持有的接口先出。
        for child in children
            .get(&container.id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            if child.kind == ResourceNodeType::Endpoint {
                out.push(child.clone());
            }
        }
        // 再把它的子集合压栈（逆序压，弹出时才是正序）。
        push_containers(&mut stack, &children, &container.id);
    }
    Ok(out)
}

/// 把 `parent_id` 的子集合按 `(sort_order, id)` 的**逆序**压进栈——栈是后进先出，
/// 逆序压入才能让弹出时是正序。
fn push_containers(
    stack: &mut Vec<ResourceNode>,
    children: &HashMap<String, Vec<ResourceNode>>,
    parent_id: &str,
) {
    let Some(kids) = children.get(parent_id) else {
        return;
    };
    let mut containers: Vec<ResourceNode> = kids
        .iter()
        .filter(|k| k.kind == ResourceNodeType::Collection)
        .cloned()
        .collect();
    containers.reverse();
    stack.extend(containers);
}

/// 一个节点的直接子节点，按显示序排列。
/// `kind` 可收窄到单一类型。在合并后的树上，这是「列出这个 collection 里的 endpoint」唯一
/// 需要的查询。
/// 非递归：要整棵子树，用 [`subtree_ids`] 配 [`list_by_root_and_type`]，或从根沿父回溯。
pub fn list_children(
    conn: &mut Connection,
    parent_id: &str,
    kind: Option<ResourceNodeType>,
) -> Result<Vec<ResourceNode>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at \
         FROM nodes WHERE parent_id = ?1 ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![parent_id], row_to_node)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    Ok(match kind {
        Some(k) => rows.into_iter().filter(|n| n.kind == k).collect(),
        None => rows,
    })
}

/// 跨全部项目按名字/描述/URL 搜索**接口**，并带上它所在的完整路径。
///
/// 只搜接口：动态变量的数据源只可能是一个接口，故选择器只需要这一类命中。路径是
/// 「项目 / 模块 / 集合…」，用户在一个很大的工作区里找一条接口时需要知道它在谁底下。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub node: ResourceNode,
    /// 从项目到直接父节点的名字，自上而下。
    pub path: Vec<String>,
}

/// 搜索接口并返回它们的路径。最多返回 `limit` 条。
pub fn search_paths(
    conn: &mut Connection,
    q: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, AppError> {
    let pattern = format!("%{q}%");
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS_N} FROM nodes n \
         LEFT JOIN endpoint_details e ON e.node_id = n.id \
         WHERE n.type = 'endpoint' \
           AND (n.name LIKE ?1 OR n.description LIKE ?1 OR e.url LIKE ?1) \
         ORDER BY n.root_id ASC, n.sort_order ASC, n.id ASC \
         LIMIT {}",
        limit.clamp(1, 200)
    ))?;
    let nodes = stmt
        .query_map(params![pattern], row_to_node)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        let path = ancestors(conn, &node)?;
        out.push(SearchHit { node, path });
    }
    Ok(out)
}

/// 一个节点的祖先名字，自上而下：先是项目名，再是模块与集合的名字。
///
/// 上溯有深度上限：数据里的环会让无上限的循环永远转下去，而一次失败的查找不该拖垮
/// 整个界面。
fn ancestors(conn: &mut Connection, node: &ResourceNode) -> Result<Vec<String>, AppError> {
    const MAX_DEPTH: usize = 64;

    let mut names: Vec<String> = Vec::new();
    let project_name: Option<String> = conn
        .query_row(
            "SELECT name FROM projects WHERE id = ?1",
            params![node.root_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(name) = project_name {
        names.push(name);
    }

    let mut chain: Vec<String> = Vec::new();
    let mut parent_id = node.parent_id.clone();
    let mut depth = 0;
    while let Some(id) = parent_id {
        depth += 1;
        if depth > MAX_DEPTH {
            break;
        }
        let parent: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT id, parent_id FROM nodes WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let Some((id, next_parent)) = parent else {
            break;
        };
        let name: Option<String> = conn
            .query_row("SELECT name FROM nodes WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .ok();
        if let Some(name) = name {
            chain.push(name);
        }
        parent_id = next_parent;
    }
    chain.reverse();
    names.extend(chain);
    Ok(names)
}

pub(crate) fn row_to_node(row: &Row<'_>) -> Result<ResourceNode, rusqlite::Error> {
    let type_str: String = row.get(3)?;
    let kind = ResourceNodeType::parse(&type_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(super::super::JsonColError(format!(
                "type: unrecognised node type {type_str:?}"
            ))),
        )
    })?;
    Ok(ResourceNode {
        id: row.get(0)?,
        root_id: row.get(1)?,
        parent_id: row.get(2)?,
        kind,
        name: row.get(4)?,
        description: row.get(5)?,
        sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
