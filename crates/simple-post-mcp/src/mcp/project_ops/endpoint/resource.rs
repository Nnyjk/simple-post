//! 把 `nodes` 行与 `endpoint_details` 行合成一个对象返回。
//!
//! 读侧 handler 每次都要这两样，所以放在一处，别让 8 个工具各写一遍 join。

use simple_post_core::domain::{EndpointResource, ResourceNode};
use simple_post_core::error::AppError;
/// node 与其请求配置合成一个对象返回。
///
/// AI 客户端问「把这个端点给我」要的是 method 与 url，而它们现在在
/// `endpoint_details` 里；只回 node 只会得到一个名字和一个类型。
pub(in crate::mcp::project_ops) fn endpoint_resource(
    pool: &simple_post_core::db::DbPool,
    node_id: &str,
) -> Result<EndpointResource, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        let node = simple_post_core::db::nodes::get(c, node_id)?;
        let detail = simple_post_core::db::endpoint_details::get(c, node_id)?;
        Ok(EndpointResource { node, detail })
    })
}

/// 给端点 node 列表配上各自的配置，保持调用方给定的 node 顺序。
pub(super) fn endpoint_resources(
    pool: &simple_post_core::db::DbPool,
    nodes: Vec<ResourceNode>,
) -> Result<Vec<EndpointResource>, AppError> {
    let ids = nodes.iter().map(|n| n.id.clone()).collect::<Vec<String>>();
    let details = simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::endpoint_details::list_by_node_ids(c, &ids)
    })?;
    let by_id = details
        .into_iter()
        .map(|d| (d.node_id.clone(), d))
        .collect::<std::collections::HashMap<_, _>>();
    Ok(nodes
        .into_iter()
        .filter_map(|node| {
            by_id.get(&node.id).map(|detail| EndpointResource {
                node,
                detail: detail.clone(),
            })
        })
        .collect())
}
