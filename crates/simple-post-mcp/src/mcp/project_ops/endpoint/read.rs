//! 读侧 handler：`endpoint_list` / `endpoint_get` / `endpoint_search`。
//!
//! 三者都要先解析真实归属项目再过门；无范围的读明确拒绝，而不是编一个 id。

use serde_json::Value;

use super::args::{EndpointGetArgs, EndpointSearchArgs};
use super::resource::{endpoint_resource, endpoint_resources};
use crate::mcp::{arg_str_opt, AppState, CallToolResult};
use simple_post_core::domain::{ResourceNode, ResourceNodeType};
use simple_post_core::error::AppError;
pub(super) async fn tool_endpoint_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str_opt(&args, "collection_id");
    let project_id = arg_str_opt(&args, "project_id");
    let module_id = arg_str_opt(&args, "module_id");

    // 按「调用方点名的东西真正归谁」过门。`collection_id` / `module_id` 指的是
    // *node*，所以 `nodes::root_id_of` 一次索引读就能拿到它的项目；裸 `project_id`
    // 本身就是答案。
    //
    // 写死字面量 "global" 不是项目 id（`agent_permissions.project_id` 是指向
    // `projects(id)` 的外键），查不到行就退回 `Permission::default()`。更糟的是按
    // 集合或模块收窄时根本没有归属项目可查，真实拥有者的权限一次也没被读过 —— 只读
    // 项目照样能被这种收窄调用读走。解析出真实归属才补上这一层。
    //
    // 一个范围都不给时，这次读确实横跨所有项目，没有单一项目可依。那就明确拒绝，
    // 而不是编一个 id：跨项目的读，按项目那扇门表达不了；拿一个项目的授权去返回
    // 另一个项目的数据，比说不行更糟。
    let owner_project = match collection_id.as_deref().or(module_id.as_deref()) {
        Some(node_id) => Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::root_id_of(c, node_id)
        })?),
        None => project_id.clone(),
    };
    let Some(owner_project) = owner_project else {
        return Err(AppError::Validation(
            "endpoint_list needs a scope: pass project_id, module_id or \
             collection_id. An unscoped read spans every project, and the \
             per-project permission gate has no single project to check."
                .into(),
        ));
    };
    crate::mcp::check_permission(&state.pool, &owner_project, "endpoint_list").await?;

    // `module_id` 收窄到模块整棵子树：节点的父链现在是同构的 —— 除模块外，每个节点
    // 都有节点父级。
    let scope = collection_id
        .as_deref()
        .map(|c| ("collection", c.to_string()))
        .or_else(|| module_id.clone().map(|m| ("module", m.to_string())))
        .or_else(|| project_id.clone().map(|p| ("project", p)));

    let rows = match scope {
        Some(("collection", cid)) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_children(c, &cid, Some(ResourceNodeType::Endpoint))
        })?,
        Some(("module", mid)) => {
            let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&mid))
            })?;
            let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
            let nodes = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::list_by_root_and_type(
                    c,
                    &owner_project,
                    ResourceNodeType::Endpoint,
                )
            })?;
            nodes
                .into_iter()
                .filter(|n| in_subtree.contains(&n.id))
                .collect()
        }
        Some((_, pid)) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(c, &pid, ResourceNodeType::Endpoint)
        })?,
        // 走不到：`scope` 为 `None` 只在什么都没点名时发生，而那种情况已被上面
        // 「没有归属可过门」的检查拒掉了。
        None => Vec::new(),
    };

    let endpoints: Vec<ResourceNode> = rows
        .into_iter()
        .filter(|n| n.kind == ResourceNodeType::Endpoint)
        .collect();
    Ok(CallToolResult::json(endpoint_resources(
        &state.pool,
        endpoints,
    )?))
}

pub(super) async fn tool_endpoint_get(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointGetArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_get args: {e}")))?;
    let ep = endpoint_resource(&state.pool, &a.endpoint_id)?;
    crate::mcp::check_permission(&state.pool, &ep.node.root_id, "endpoint_get").await?;
    Ok(CallToolResult::json(ep))
}

pub(super) async fn tool_endpoint_search(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointSearchArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_search args: {e}")))?;

    // 归属解析与 `endpoint_list` 完全一致。字面量 "global" 不是项目 id（那一列是指向
    // `projects(id)` 的外键），查不到行就静默退回默认值；按集合或模块收窄时更糟，
    // 真实拥有者的权限根本没被查过。
    //
    // 一个范围都不给就横跨项目，没有单一归属可授权，同样明确拒绝而不是编 id。
    // 另外注意这里传的是**本工具**的名字：传别的名字会让针对搜索的权限改动被拿去
    // 比另一行。
    let owner_project = match a.collection_id.as_deref().or(a.module_id.as_deref()) {
        Some(node_id) => Some(simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::root_id_of(c, node_id)
        })?),
        None => a.project_id.clone(),
    };
    let Some(owner_project) = owner_project else {
        return Err(AppError::Validation(
            "endpoint_search needs a scope: pass project_id, module_id or \
             collection_id. An unscoped search spans every project, and the \
             per-project permission gate has no single project to check."
                .into(),
        ));
    };
    crate::mcp::check_permission(&state.pool, &owner_project, "endpoint_search").await?;

    // `method` 与 `tags` 住在 `endpoint_details`，所以自由文本匹配与这两个过滤都放在
    // 收窄到端点 node 之后 —— SQL 层只匹配 name / url / description。
    let mut nodes = match a.q.as_deref().filter(|q| !q.is_empty()) {
        Some(q) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::search(c, q)
        })?,
        None => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(
                c,
                &owner_project,
                ResourceNodeType::Endpoint,
            )
        })?,
    };
    // `module_id` / `collection_id` 范围必须也收窄自由文本那一支，不只是无过滤那一支
    // —— 否则门查的是模块所属项目，返回的结果却横跨所有项目，正是这道门要挡的
    // 跨项目渗漏。
    if a.q.as_deref().filter(|q| !q.is_empty()).is_some() {
        if let Some(scope_id) = a.collection_id.as_deref().or(a.module_id.as_deref()) {
            let scope_id = scope_id.to_string();
            let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
                simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&scope_id))
            })?;
            let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
            nodes.retain(|n| in_subtree.contains(&n.id));
        } else {
            nodes.retain(|n| n.root_id == owner_project);
        }
    }
    nodes.retain(|n| n.kind == ResourceNodeType::Endpoint);
    let mut out = endpoint_resources(&state.pool, nodes)?;

    if let Some(m) = a.method.as_deref() {
        let want = simple_post_core::domain::parse_http_method(m)?;
        out.retain(|r| r.detail.method == want);
    }
    if let Some(tags) = a.tags.as_deref() {
        out.retain(|r| tags.iter().all(|t| r.detail.tags.contains(t)));
    }
    Ok(CallToolResult::json(out))
}
