//! 项目与资源树节点的创建编排 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 三个创建入口的编排只有这一处实现。Tauri 命令在 `src-tauri/src/commands/`，REST 路由在
//! `crates/simple-post-server/src/web_api/routes/`，两侧都只留薄壳：薄壳负责**线格式**
//! （REST 的两种 body 形状、Tauri 的 `kind` 字符串解析），本模块负责**落库**（id 归谁铸、
//! 时间戳、父子关系的推导、事务边界）。
//!
//! 与 `spec_api` / `dynamic_api` / `interchange_api` 同形，但那三个模块的门面在
//! `simple_post_mcp` 里（MCP 工具也在调它们）；这三段编排没有 MCP 工具在调 —— MCP
//! 那边的 `project_create` 走的是 `core::spec` 的「按名找或建」（带 `if_exists`），与这里的
//! 「照单新建」是两种契约，故不在本模块范围内。
//!
//! ## 不查逐项目权限档位
//!
//! `permissions` 表管的是「AI 客户端在这个项目里能做什么」，管它的是 MCP 那条路。
//! REST 路由与桌面命令是用户自己的会话，与 `nodes_create` 同一套规矩：写，但不查
//! 那一档。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{Project, ResourceNode};
use simple_post_core::error::AppError;

use crate::dto::{CreateEndpointDetailOpts, CreateProjectOpts};

/// 建一个节点要什么。
///
/// 结构那一半（节点行本身）由调用方按自己的线上形状造好，`detail` 是 endpoint 才带的
/// 那半边请求配置。两半一起写进一个事务：缺 detail 行的节点、或缺节点的 detail 行，
/// 都是读者走不动的树。
pub struct NodeCreateInput {
    /// 节点行。`root_id` 留空时由 [`create_node`] 按 `parent_id` 推导。
    pub node: ResourceNode,
    /// endpoint 才带的那一半请求配置。其余节点必须是 `None`。
    pub detail: Option<CreateEndpointDetailOpts>,
}

/// 建项目，并种下它的默认环境。
///
/// `id` 是调用方给的 id：给了非空的就照用，空的与缺席都由服务端铸一个。REST 侧照旧
/// 先经 `ensure_server_id` 把 body 补齐，Tauri 命令没有 id 参数、直接传 `None`。
///
/// 返回库里那一行而非入参：项目落库时会种下默认环境与默认模块，读回来的那一行才与
/// 数据库此刻持有的内容一致。
pub fn create_project(
    pool: &DbPool,
    id: Option<String>,
    name: String,
    opts: CreateProjectOpts,
) -> Result<Project, AppError> {
    let now = crate::domain::now_ms();
    let project = Project {
        id: adopt_non_empty(id),
        name,
        description: opts.description,
        color: opts
            .color
            .unwrap_or_else(crate::domain::default_project_color),
        created_at: now,
        updated_at: now,
    };
    crate::db::with_tx(pool, |c| {
        crate::db::projects::create_with_defaults(c, &project)
    })
}

/// 建节点。endpoint 一次调用带上两半（结构节点 + 请求配置），两者写在同一个事务里，
/// 而任何一半可能缺失的树是打不开的树。
///
/// `root_id` 留空时按 `parent_id` 推导：子节点的归属由父节点蕴含，调用方只需点名父
/// 节点，两者不可能打架。
pub fn create_node(pool: &DbPool, input: NodeCreateInput) -> Result<ResourceNode, AppError> {
    let NodeCreateInput { mut node, detail } = input;
    // MUTATION-2：一律从父节点推导，忽略调用方给的 root_id
    if node.root_id.is_empty() {
        node.root_id = resolve_root_id(pool, None, node.parent_id.as_deref())?;
    }
    let detail = detail.map(|d| d.into_detail(&node.id));
    crate::db::with_tx(pool, |c| {
        crate::db::nodes::create(c, &node, detail.as_ref())
    })
}

/// 这个节点属于哪个项目。
///
/// 调用方点名了 `root_id` 就用它；没点名而点名了 `parent_id`，就从父节点那里查出来。
/// 两者都没有时是空串——没有格子可读，不是「猜一个」。
///
/// 只在 `root_id` 缺席时查库：给了 `root_id` 的调用方已经回答了这个问题。
pub fn resolve_root_id(
    pool: &DbPool,
    root_id: Option<&str>,
    parent_id: Option<&str>,
) -> Result<String, AppError> {
    let given = root_id.unwrap_or_default();
    if !given.is_empty() {
        return Ok(given.to_string());
    }
    match parent_id {
        Some(parent) => crate::db::with_conn(pool, |c| crate::db::nodes::root_id_of(c, parent)),
        None => Ok(String::new()),
    }
}

/// 调用方给的 id：非空就照用，空的与缺席都铸一个。
///
/// 纯空白也算「给了」：项目 id 是调用方给的标识，它给什么就是什么。
fn adopt_non_empty(id: Option<String>) -> String {
    match id {
        Some(id) if !id.is_empty() => id,
        _ => crate::domain::uid(),
    }
}

#[cfg(test)]
#[path = "project_api_test.rs"]
mod tests;

#[cfg(test)]
#[path = "project_api_shells_test.rs"]
mod shells_tests;
