//! `domain → ir` 投影：把资源树读成 IR，供导出。
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
//! 也不落回项目 ★。故两个入口都要求调用方指名 `environment_id`（`None` 表示取激活环境）：
//! 让「用哪个环境」成为一次显式的决定，而不是本模块替用户挑一个。

use std::collections::HashMap;

use simple_post_core::db::{self, Connection, DbPool};
use simple_post_core::domain::{
    BaseUrlDefinition, EndpointDetail, Environment, Project, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrServer};

/// 一次投影读回来的全部上下文。两个导出入口的差别只在「取哪些根节点」，
/// 故四张表的读与「不回落」的服务解析都共用这一份。
struct Projection {
    project: Project,
    env: Option<Environment>,
    details: HashMap<String, EndpointDetail>,
    children_of: HashMap<Option<String>, Vec<ResourceNode>>,
}

impl Projection {
    /// 组装最终那份 [`Exchange`]。`environment_id` 是**实际用到的**那个环境的 id：
    /// 指名的环境不属于该项目时 `env` 为 `None`，这份 IR 里每个模块的 `servers` 都是空的
    /// ——那正是「这些模块没有 URL」的忠实答案，而不是「环境没选」。
    fn into_exchange(self, modules: Vec<IrModule>, diagnostics: Vec<Diagnostic>) -> Exchange {
        Exchange {
            project_name: self.project.name,
            project_description: self.project.description,
            environment_id: self.env.as_ref().map(|e| e.id.clone()),
            modules,
            diagnostics,
        }
    }
}

/// 四张表读成一份投影。
fn load_projection(
    conn: &mut Connection,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<Projection, AppError> {
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

    Ok(Projection {
        project,
        env,
        details,
        children_of,
    })
}

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
        let p = load_projection(conn, project_id, environment_id)?;
        let roots = p.children_of.get(&None).cloned().unwrap_or_default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let modules = roots
            .iter()
            .filter(|n| n.kind == ResourceNodeType::Module)
            .map(|m| {
                project_module(
                    m,
                    &p.project,
                    p.env.as_ref(),
                    &p.details,
                    &p.children_of,
                    &mut diagnostics,
                )
            })
            .collect();
        Ok(p.into_exchange(modules, diagnostics))
    })
}

/// 把**一个模块**的整棵子树投影成一份只含一个模块的 [`Exchange`]。导出入口用这个：
/// 落点是模块行上的 `+`，范围就是那棵子树。
///
/// `module_id` 必须属于 `project_id` 且必须是模块——两种错都要报得出，因为调用方传的是
/// 一个界面上选的 id，而它可能已经被删掉，或根本不是模块。
pub fn module_to_ir(
    pool: &DbPool,
    project_id: &str,
    module_id: &str,
    environment_id: Option<&str>,
) -> Result<Exchange, AppError> {
    db::with_conn(pool, |conn| {
        let node = db::nodes::get(conn, module_id)?;
        if node.root_id != project_id {
            return Err(AppError::NotFound(format!(
                "模块 {module_id} 不属于项目 {project_id}"
            )));
        }
        if node.kind != ResourceNodeType::Module {
            return Err(AppError::Validation(format!(
                "导出落点必须是模块，而 {module_id} 是一个 {}",
                node.kind.as_str()
            )));
        }

        let p = load_projection(conn, project_id, environment_id)?;
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let modules = vec![project_module(
            &node,
            &p.project,
            p.env.as_ref(),
            &p.details,
            &p.children_of,
            &mut diagnostics,
        )];
        Ok(p.into_exchange(modules, diagnostics))
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
        // 导出侧的 `key` 就是节点 id：勾选集要能换回具体是哪一行。
        key: node.id.clone(),
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
                    key: n.id.clone(),
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
        key: node.id.clone(),
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
