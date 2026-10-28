// ===========================================================================
// region: nodes
// ===========================================================================
//
// Tauri 半边；REST 半边在 `web_api/routes/nodes.rs`，两者都过 `crate::db::nodes`，
// 两种传输不可能分叉。
//
// 非结构的两类命令另有归属：`nodes_duplicate` 与 endpoint-detail 一对。

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::CreateEndpointDetailOpts;

/// 某项目下的全部节点；不给 `root_id` 则全库。
///
/// `kind` 收窄到单一类型，`query` 走子串搜索（要 join `endpoint_details`，`url` 在那）。
/// `query` 优先于 `kind`：跨类型搜索是调用方自己就能加的过滤，后端替它在两者间猜才是猜。
#[tauri::command]
pub fn nodes_list(
    state: State<DbPool>,
    root_id: Option<String>,
    kind: Option<String>,
    query: Option<String>,
) -> Result<Vec<ResourceNode>, AppError> {
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        return crate::db::with_conn(&state, |c| crate::db::nodes::search(c, &q));
    }
    let kind = match kind.filter(|k| !k.is_empty()) {
        Some(k) => Some(
            ResourceNodeType::parse(&k)
                .ok_or_else(|| AppError::Validation(format!("unknown node type {k:?}")))?,
        ),
        None => None,
    };
    // 只有类型过滤而无 root 时，查询会扫全库再丢掉大半；命令直接报错，免得悄悄返回
    // 另一个项目的树。
    if kind.is_some() && root_id.is_none() {
        return Err(AppError::Validation("type filter requires rootId".into()));
    }
    crate::db::with_conn(&state, |c| match (kind, root_id.as_deref()) {
        (Some(k), Some(root)) => crate::db::nodes::list_by_root_and_type(c, root, k),
        (None, Some(root)) => crate::db::nodes::list_by_root(c, root),
        (None, None) => crate::db::nodes::list_all(c),
        // 上面已拦下这组组合（给了 kind 必给 root_id），这个分支走不到。
        (Some(_), None) => Ok(Vec::new()),
    })
}

#[tauri::command]
pub fn nodes_get(state: State<DbPool>, id: String) -> Result<ResourceNode, AppError> {
    crate::db::with_conn(&state, |c| crate::db::nodes::get(c, &id))
}

/// 一个节点的直接子节点。懒加载层级树的一节：展开时才取，不预先拉整棵树。
///
/// `parent_id` 给一个节点就取它的孩子；`project_id` 给一个项目就取那个项目的模块
/// （模块的父是项目，而项目不是节点，故不能按 `parent_id` 找）。
#[tauri::command]
pub fn nodes_children(
    state: State<DbPool>,
    parent_id: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<ResourceNode>, AppError> {
    crate::db::with_conn(&state, |c| match (parent_id, project_id) {
        (Some(parent), _) => crate::db::nodes::list_children(c, &parent, None),
        (None, Some(project)) => {
            crate::db::nodes::list_by_root_and_type(c, &project, ResourceNodeType::Module)
        }
        (None, None) => Err(AppError::Validation(
            "parentId or projectId is required".into(),
        )),
    })
}

/// 跨全部项目搜接口，带它所在的路径（项目 / 模块 / 集合…）。
#[tauri::command]
pub fn nodes_search_paths(
    state: State<DbPool>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<crate::db::nodes::SearchHit>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::nodes::search_paths(c, &query, limit.unwrap_or(50))
    })
}

/// 建节点。endpoint 一次调用带上两半（结构节点 + 请求配置）：两者写在同一个事务里，
/// 而任何一半可能缺失的树是打不开的树。
///
/// 只有 module 需要 `root_id`；collection / endpoint 从父节点推出，两者不可能打架。
///
/// 命令这一侧只负责线格式：`kind` 是字符串，在这里解析成枚举。
#[tauri::command]
pub fn nodes_create(
    state: State<DbPool>,
    root_id: Option<String>,
    parent_id: Option<String>,
    kind: String,
    name: String,
    description: Option<String>,
    detail: Option<CreateEndpointDetailOpts>,
) -> Result<ResourceNode, AppError> {
    let kind = ResourceNodeType::parse(&kind)
        .ok_or_else(|| AppError::Validation(format!("unknown node type {kind:?}")))?;
    let now = crate::domain::now_ms();
    let node = ResourceNode {
        id: uid(),
        root_id: root_id.unwrap_or_default(),
        parent_id,
        kind,
        name,
        description,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    crate::project_api::create_node(&state, crate::project_api::NodeCreateInput { node, detail })
}

/// 改名 / 改描述 / 改顺序。移动另立一个命令（见 [`nodes_move`]），因为它得保住
/// 类型矩阵与整棵子树的 `root_id`。
#[tauri::command]
pub fn nodes_update(
    state: State<DbPool>,
    id: String,
    patch: ResourceNodePatch,
) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::update(c, &id, patch.clone()))
}

/// 删一个节点与它整棵子树。
#[tauri::command]
pub fn nodes_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::delete(c, &id))
}

/// 移动节点到新父节点下的新同级位置。
///
/// `parent_id` 为 `None` 是「项目顶层」，只对 module 合法——目标父节点自身的类型
/// 已经回答了「放哪」，无需另带 `parentKind`。`node_ids` 即落地顺序，多选拖拽得以
/// 保持用户的选择次序。
#[tauri::command]
pub fn nodes_move(
    state: State<DbPool>,
    node_ids: Vec<String>,
    parent_id: Option<String>,
    index: i32,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::move_many(c, &node_ids, parent_id.as_deref(), index)
    })
}

/// 在同父下、紧跟其后复制一个 endpoint。
///
/// 只限 endpoint：module / collection 的「副本」是空壳，复制子树是另一个功能而非复制。
///
/// `new_id` 由调用方给就照用，让乐观复制能立刻给标签栏一行它渲染时确实存在的记录；
/// 不给则服务端生成，没有本地副本要对齐的一方（MCP 工具）要的就是后者。
#[tauri::command]
pub fn nodes_duplicate(
    state: State<DbPool>,
    id: String,
    new_id: Option<String>,
) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::duplicate(c, &id, new_id.as_deref())
    })
}

/// 一个 endpoint 节点的请求配置。
#[tauri::command]
pub fn endpoint_detail_get(
    state: State<DbPool>,
    node_id: String,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_conn(&state, |c| crate::db::endpoint_details::get(c, &node_id))
}

/// 全部 endpoint 的请求配置，扁平列表，渲染侧按 `nodeId` 建索引。
///
/// 与 [`nodes_list`] 分开：树不渲染 method / url，只需要名称 / 类型 / 顺序。启动时为
/// 每个节点读一次 detail 表，会拉来多数行根本没有的列集，而折进树读取就是每次启动
/// 一次 join，换来用户可能一辈子不打开的数据。
#[tauri::command]
pub fn endpoint_details_list(state: State<DbPool>) -> Result<Vec<EndpointDetail>, AppError> {
    // 每个项目一次查询而非 join：`endpoint_details` 以节点 id 为键，要触达某项目的
    // endpoint 只能经 `nodes.root_id`；`list_by_root` 已含这个 join，项目数受用户
    // 实际打开的范围约束。
    let roots = crate::db::with_conn(&state, crate::db::projects::list)?
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<String>>();
    let mut out = Vec::new();
    for root in roots {
        out.extend(crate::db::with_conn(&state, |c| {
            crate::db::endpoint_details::list_by_root(c, &root)
        })?);
    }
    Ok(out)
}

/// 改 endpoint 的请求配置。与 [`nodes_update`] 分开，免得改个名字就重写没变的 JSON 列。
#[tauri::command]
pub fn endpoint_detail_update(
    state: State<DbPool>,
    node_id: String,
    patch: EndpointDetailPatch,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoint_details::update(c, &node_id, patch.clone())
    })
}
