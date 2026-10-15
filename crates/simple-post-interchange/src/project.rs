//! `domain → ir` 投影：把一个项目读成 IR，供导出。
//!
//! ## 为什么这里要跨四张表
//!
//! host 不在 endpoint 上。URL 住在 `environments.module_base_urls_json`，按
//! `service_id → module_id` 二级索引；「这个 endpoint 指哪个服务」在
//! `endpoint_details.base_url_definition_id`，「项目的 ★」在
//! `projects.default_base_url_definition_id`。加上 `nodes` 的树，四张表才是完整的请求；
//! 任何一行都不足以回答「这个接口该打到哪里去」。
//!
//! **先选定环境，再逐模块取，不回退。** 该模块没配就是没有 URL——不借用别的模块的，
//! 也不落回项目 ★。故 [`project_to_ir`] 要求调用方指名 `environment_id`：
//! 让「用哪个环境」成为一次显式的决定，而不是本模块替用户挑一个。

use std::collections::HashMap;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    BaseUrlDefinition, EndpointDetail, Environment, Project, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrServer};

/// 把项目 `project_id` 在环境 `environment_id` 下的样子投影成一份 [`Exchange`]。
///
/// `environment_id` 为 `None` 时取该项目的激活环境；连激活环境都没有（用户把唯一的环境
/// 停用了）时**不猜**，产出的 IR 里每个模块的 `servers` 都是空的——那正是
/// 「这些模块没有 URL」的忠实答案。
pub fn project_to_ir(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<Exchange, AppError> {
    db::with_conn(pool, |conn| {
        let project = db::projects::get(conn, project_id)?;

        // 指名的环境可能不属于该项目：那会让 URL 跨项目串过来，故只接受本项目的环境。
        let env = match environment_id {
            Some(id) => Some(db::environments::get(conn, id)?),
            None => db::environments::get_active(conn, project_id)?,
        };
        let env = env.filter(|e| e.project_id == project_id);

        // 一次读回整棵树，内存里按 parent_id 分桶：导出是一次投影，
        // 逐节点发查询会让一棵大树变成 N 次往返。
        let nodes = db::nodes::list_by_root(conn, project_id)?;
        let mut children_of: HashMap<Option<String>, Vec<ResourceNode>> = HashMap::new();
        for n in nodes {
            children_of.entry(n.parent_id.clone()).or_default().push(n);
        }
        // 每桶内的顺序即显示顺序，与 `list_by_root` 的 ORDER BY 保持一致。
        for list in children_of.values_mut() {
            list.sort_by(|a, b| {
                a.sort_order
                    .cmp(&b.sort_order)
                    .then_with(|| a.id.cmp(&b.id))
            });
        }

        let details: HashMap<String, EndpointDetail> =
            db::endpoint_details::list_by_root(conn, project_id)?
                .into_iter()
                .map(|d| (d.node_id.clone(), d))
                .collect();

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let roots = children_of.remove(&None).unwrap_or_default();
        let modules = roots
            .iter()
            .filter(|n| n.kind == ResourceNodeType::Module)
            .map(|m| {
                project_module(
                    m,
                    &project,
                    env.as_ref(),
                    &details,
                    &children_of,
                    &mut diagnostics,
                )
            })
            .collect();

        Ok(Exchange {
            project_name: project.name,
            project_description: project.description,
            environment_id: env.as_ref().map(|e| e.id.clone()),
            modules,
            diagnostics,
        })
    })
}

/// 一个模块节点及其整棵子树。collection 嵌套深度无上限，故这里递归。
#[allow(clippy::too_many_arguments)]
fn project_module(
    node: &ResourceNode,
    project: &Project,
    env: Option<&Environment>,
    details: &HashMap<String, EndpointDetail>,
    children_of: &HashMap<Option<String>, Vec<ResourceNode>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> IrModule {
    IrModule {
        name: node.name.clone(),
        description: node.description.clone(),
        servers: module_servers(&project.base_url_definitions, env, node, diagnostics),
        endpoints: project_endpoints(node.id.as_str(), details, children_of, diagnostics),
        collections: project_collections(node.id.as_str(), details, children_of, diagnostics),
    }
}

/// 容器节点下的 endpoint 与 collection。两者分开取，因为树里先渲染分组再渲染接口，
/// 而 `list_by_root` 只保证全局有序。
fn project_endpoints(
    parent_id: &str,
    details: &HashMap<String, EndpointDetail>,
    children_of: &HashMap<Option<String>, Vec<ResourceNode>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<IrEndpoint> {
    children_of
        .get(&Some(parent_id.to_string()))
        .map(|c| {
            c.iter()
                .filter(|n| n.kind == ResourceNodeType::Endpoint)
                .filter_map(|n| project_endpoint(n, details, diagnostics))
                .collect()
        })
        .unwrap_or_default()
}

fn project_collections(
    parent_id: &str,
    details: &HashMap<String, EndpointDetail>,
    children_of: &HashMap<Option<String>, Vec<ResourceNode>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<IrCollection> {
    children_of
        .get(&Some(parent_id.to_string()))
        .map(|c| {
            c.iter()
                .filter(|n| n.kind == ResourceNodeType::Collection)
                .map(|n| IrCollection {
                    name: n.name.clone(),
                    description: n.description.clone(),
                    endpoints: project_endpoints(&n.id, details, children_of, diagnostics),
                    collections: project_collections(&n.id, details, children_of, diagnostics),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 一个 endpoint 行 + 它的 detail 行。缺 detail 行意味着两表不一致，
/// 报出来好过把默认值悄悄补上——用户看到的会是一个打不出请求的接口。
fn project_endpoint(
    node: &ResourceNode,
    details: &HashMap<String, EndpointDetail>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<IrEndpoint> {
    let d = details.get(&node.id)?;
    if d.node_id != node.id {
        diagnostics.push(Diagnostic::error(
            format!("/nodes/{}", node.id),
            "节点与它的 endpoint detail 对不上，该接口无法导出",
        ));
        return None;
    }
    Some(IrEndpoint {
        name: node.name.clone(),
        description: node.description.clone(),
        method: d.method.clone(),
        path: d.url.clone(),
        params: d.params.clone(),
        headers: d.headers.clone(),
        body: d.body.clone(),
        auth: d.auth.clone(),
        docs: d.docs.clone(),
        notes: d.notes.clone(),
        tags: d.tags.clone(),
    })
}

/// 模块在该环境下、逐服务取的 URL。**不回退**：单元格缺项即「该模块对此服务无地址」。
///
/// 服务表是项目级的，故按项目已有的定义顺序输出，导出的 `servers` 顺序因而稳定。
fn module_servers(
    defs: &[BaseUrlDefinition],
    env: Option<&Environment>,
    module: &ResourceNode,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<IrServer> {
    let Some(env) = env else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for def in defs {
        let Some(per_module) = env.module_base_urls.get(&def.id) else {
            continue;
        };
        match per_module.get(&module.id) {
            Some(url) => out.push(IrServer {
                service: def.name.clone(),
                url: url.clone(),
            }),
            // 没有回退层，所以这不是「借用项目 ★」的位置；只在 ★ 恰好指着这个服务
            // 且本模块配了值时才有意义，否则不产出条目。
            None => diagnostics.push(Diagnostic::info(
                format!("/environments/{}/moduleBaseUrls/{}", env.id, module.id),
                format!("模块「{}」未配置服务「{}」的 URL", module.name, def.name),
            )),
        }
    }
    out
}

// 测试——投影的跨表取值与「不回退」。

#[cfg(test)]
#[path = "project_test.rs"]
mod tests;
