//! `nodes` 表的 CRUD——合并后的资源树。
//!
//! 每个节点一行，由 `type` 区分、以 `parent_id` 相连；`root_id` 反范式化出所属项目，
//! 故整棵树是一次走索引的读，「这属于哪个项目」也成了一次主键命中，而不必沿父链上溯。
//!
//! ## 合并丢了什么，由什么接管
//!
//! 三表布局靠外键（如 `endpoints.collection_id REFERENCES collections(id)`）免费得到
//! 「父永远接受它的列能指名的那些类型」。单一多态 `parent_id` 做不到：SQLite 的 CHECK
//! 约束读不到别的行。
//!
//! 故父子矩阵住在 [`ResourceNodeType::may_under`]，并在此处每次写入时执行，`schema.sql`
//! 另有配套触发器作为第二道防线。触发器那半不是冗余的双保险：MCP 工具是本项目主要的外部
//! 写路径，跳过 `db::nodes` 的将来调用者否则可以自由搭出一棵没有读者走得动的树。
//!
//! ## `root_id` 是推导值，推导值需要维护
//!
//! 它换来的是 `list_by_root` 一次走索引的扫描、`root_id_of` 一次主键查找——逐项目权限门的
//! 第一步。代价是跨项目边界的移动必须为整棵子树重新推导，而不只是调用方指名的那几行。
//! [`move_many`] 做了这件事，`subtree_module_id_cascade_has_a_test` 是回归网。

use rusqlite::{params, Connection, Row};

use crate::domain::{EndpointDetail, ResourceNode, ResourceNodePatch, ResourceNodeType};
use crate::error::AppError;

const COLUMNS: &str = "id, root_id, parent_id, type, name, description, sort_order, \
                       created_at, updated_at";

/// 同一份列表，为要 join 第二张表的查询加上表限定。
const COLUMNS_N: &str = "n.id, n.root_id, n.parent_id, n.type, n.name, n.description, \
                         n.sort_order, n.created_at, n.updated_at";

// 读。

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
    let detail = super::endpoint_details::get(conn, id)?;
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
/// 现役值层是 `moduleBaseUrls[service][module]`，故解析一个 endpoint 的 URL 是两次查找
/// ——哪个服务、哪个模块——此函数是第二次。这是树上的问题而非该行自身的属性，故不像
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

// 写。

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
        super::endpoint_details::create(conn, d)?;
    }
    Ok(node.clone())
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
    let index = sibling_index(conn, &node, id)?;
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

// 内部实现。

/// 写入前先检查一次放置是否合法。四种可能出错的地方：
/// * 挂在一个类型不接受它的父下
/// * 模块被给了节点父
/// * `root_id` 与父的不一致——那会让节点归档在一个项目下、却渲染在另一个项目的树里
/// * 顶层节点没有指名项目，或指名的项目不存在
///
/// 最后一条尤其要紧：两条传输层都把缺席的 `rootId` 默认成空串，而顶层节点没有 `parentId`
/// 可供推导，空串于是直达 `INSERT`，SQLite 报 `FOREIGN KEY constraint failed`——那会以
/// 500 加 `code: "sqlite"` 出现，把一个调用方的错误说成传输故障，而错误封装无法让客户端
/// 把它与真正的服务端故障区分开。
fn validate_placement(
    conn: &mut Connection,
    kind: ResourceNodeType,
    parent_id: Option<&str>,
    root_id: &str,
) -> Result<(), AppError> {
    let Some(parent_id) = parent_id else {
        if kind != ResourceNodeType::Module {
            return Err(AppError::Validation(format!(
                "a {} needs a parent node; only a module can sit at the top of a project",
                kind.as_str()
            )));
        }
        // 模块的父是项目本身，行无法引用它——所以项目必须以某种方式被指名。
        if root_id.is_empty() {
            return Err(AppError::Validation(
                "a top-level node needs its project: pass rootId, or a parentId to derive it from"
                    .into(),
            ));
        }
        if crate::db::projects::get(conn, root_id).is_err() {
            return Err(AppError::Validation(format!(
                "no project with id {root_id:?}; a top-level node must name the project it belongs to"
            )));
        }
        return Ok(());
    };

    let parent = get(conn, parent_id)?;
    if !kind.may_under(parent.kind) {
        return Err(AppError::Validation(format!(
            "a {} cannot sit under a {}",
            kind.as_str(),
            parent.kind.as_str()
        )));
    }
    if parent.root_id != root_id {
        return Err(AppError::Validation(format!(
            "node belongs to project {} but its parent belongs to {}",
            root_id, parent.root_id
        )));
    }
    Ok(())
}

/// 一个 endpoint 恰有一行 detail；module 或 collection 不得有。
/// 前置检查，好让忘传——或给 collection 传了 detail 的——调用方拿到一条指名错误的校验错误，
/// 而不是一棵打开该条目时报告缺行的树。
fn check_detail_agreement(
    node: &ResourceNode,
    detail: Option<&EndpointDetail>,
) -> Result<(), AppError> {
    match (node.kind, detail) {
        // 顺序要紧：合法的 endpoint 分支必须先于下面那条「不允许 detail」的分支匹配，否则被它吞掉。
        (ResourceNodeType::Endpoint, Some(d)) if d.node_id == node.id => Ok(()),
        (ResourceNodeType::Endpoint, Some(EndpointDetail { node_id, .. })) => {
            Err(AppError::Validation(format!(
                "endpoint detail is for node {node_id} but the node is {}",
                node.id
            )))
        }
        (ResourceNodeType::Endpoint, None) => Err(AppError::Validation(format!(
            "endpoint node {} needs an endpoint detail payload",
            node.id
        ))),
        (_, Some(_)) => Err(AppError::Validation(format!(
            "a {} node takes no endpoint detail payload",
            node.kind.as_str()
        ))),
        (_, None) => Ok(()),
    }
}

/// `id` 在它当前兄弟中的零基位置。
fn sibling_index(conn: &mut Connection, node: &ResourceNode, id: &str) -> Result<i32, AppError> {
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

fn row_to_node(row: &Row<'_>) -> Result<ResourceNode, rusqlite::Error> {
    let type_str: String = row.get(3)?;
    let kind = ResourceNodeType::parse(&type_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(super::JsonColError(format!(
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

// 测试。

#[cfg(test)]
#[path = "nodes_test.rs"]
mod tests;
