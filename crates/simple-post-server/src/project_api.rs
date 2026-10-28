//! 项目、服务与资源树节点的创建编排 —— REST 与 Tauri 两侧 transport 共用。
//!
//! 四个创建入口的编排只有这一处实现。Tauri 命令在 `src-tauri/src/commands/`，REST 路由在
//! `crates/simple-post-server/src/web_api/routes/`，两侧都只留薄壳：薄壳负责**线格式**
//! （REST 的两种 body 形状、Tauri 的 `kind` 字符串解析），本模块负责**落库**（id 归谁铸、
//! 时间戳、父子关系的推导、事务边界）。
//!
//! 与 `spec_api` / `dynamic_api` / `interchange_api` 同形，但那三个模块的门面在
//! `simple_post_mcp` 里（MCP 工具也在调它们）；这四段编排没有 MCP 工具在调 —— MCP
//! 那边的 `project_create` 与 `base_url_def_create` 走的是 `core::spec` 的「按名找或建」
//! （带 `if_exists`），与这里的「照单新建」是两种契约，故不在本模块范围内。按项目的
//! 权限门只有 `base_url_def_create` 有——`project_create` 刻意不走：项目还不存在，
//! 没有归属项目，也就没有可套用的存档权限。
//!
//! ## 不查逐项目权限档位
//!
//! `permissions` 表管的是「AI 客户端在这个项目里能做什么」，管它的是 MCP 那条路。
//! REST 路由与桌面命令是用户自己的会话，与 `nodes_create` 同一套规矩：写，但不查
//! 那一档。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{BaseUrlDefinition, Project, ResourceNode};
use simple_post_core::error::AppError;

use crate::dto::{adopt_or_mint_id, CreateEndpointDetailOpts, CreateProjectOpts};

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
        // base URL 不在项目上：它住在（环境 × 模块 × 服务）三者的交叉格里，而新建
        // 项目时用户还没起服务名。
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
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

/// 往**项目**的服务列表里加一条。
///
/// 项目里还没有 ★ 就仍是 `None`：建服务不等于选一个，用户没挑过默认的项目就该在没有
/// base URL 的状态下发出去，而不是带上一个他从未选过的地址。
///
/// 服务 id 归谁铸由 [`adopt_or_mint_id`] 定：调用方给了非空的就照用（渲染进程在
/// 这一行存在之前就拿这个 id 当项目 ★ 与每格 URL 的键），否则服务端铸一个。
///
/// 返回库里那一行而非入参：渲染侧要把它换进自己那份列表，只有重读才与数据库此刻
/// 持有的内容一致。
pub fn create_base_url_def(
    pool: &DbPool,
    project_id: &str,
    id: Option<String>,
    name: String,
    description: Option<String>,
) -> Result<BaseUrlDefinition, AppError> {
    let def = BaseUrlDefinition {
        id: adopt_or_mint_id(id),
        name,
        description,
    };
    crate::db::with_tx(pool, |c| {
        crate::db::base_url_defs::create(c, &def, project_id)?;
        crate::db::base_url_defs::get(c, &def.id)
    })
}

/// 把项目的 ★ 指到它自己的某个服务上，或收回（`def_id = None`）。
///
/// 返回 `Project` 而非空：`defaultBaseUrlDefinitionId` 是活的，没指名服务的 endpoint
/// 按它解析，返回空会让调用方从「无」猜出新值，再重读一次确认写入。
pub fn set_default_base_url_def(
    pool: &DbPool,
    project_id: &str,
    def_id: Option<&str>,
) -> Result<Project, AppError> {
    crate::db::with_tx(pool, |c| {
        crate::db::base_url_defs::set_default(c, project_id, def_id)?;
        crate::db::projects::get(c, project_id)
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
/// 与 `crate::dto::adopt_or_mint_id` 的差别只在一处：它把纯空白也当缺失（服务 id
/// 会进 URL 与文件名），这里不——项目 id 是调用方给的标识，它给什么就是什么。
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
