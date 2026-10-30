//! spec —— 「按名找到或建出来」的寻址原语。
//!
//! ## 为什么要有这一层
//!
//! MCP 工具是 CRUD 粒度的：建模块、建集合、建端点各是一个工具，而且都只收 id。
//! 于是「创建一个接口规范」这种跨多类实体、有先后依赖的复合任务，被拆成
//! 「先查一遍、再建一下」的十几步；每一步查出来的 id 是下一步的输入，所以它们
//! 天然串行。同一件事在 REST 路由与桌面 command 侧还要再写一遍。
//!
//! 本模块把「找到就复用、找不到就建」这一步收在一处，并让三个宿主
//! （`simple-post-server` 的 REST 路由、`simple-post-mcp` 的工具、Tauri command）
//! 拿到同一套语义。**它只做寻址**：拼请求、发请求、解析响应都不在这里。
//!
//! ## 幂等靠什么判定
//!
//! 按**名字**，不按指纹。指纹（method + path + body 摘要）是 interchange 那边的
//! 概念，core 不依赖 interchange，故 core 侧不另造一套；名字是调用方看得见、
//! 说得清的东西，「这个模块叫 trade」是一个可以复述的判据。
//!
//! 端点的例外：端点的名字不唯一（两个集合下各有一个叫「查询」的接口是常事），
//! 所以端点按**同一个父下 method + url 完全相同**来判定。这是调用方能自己
//! 复述出来的判据，不是一串指纹。
//!
//! ## 本模块的三部分
//!
//! * 本文件：寻址原语 —— 「按名找到或建出来」。
//! * [`input`]：agent 的输入 → 域类型的强制转换。
//! * [`upsert`]：一份接口规格的线格式与整份落库的编排。
//!
//! 三部分都在 core，因为它们要在 REST 路由、桌面 command 与 MCP 工具三处保持
//! 同一套语义；写在某一侧，另一侧就会静默少一半。

pub mod input;
pub mod upsert;

use crate::db::{self, Connection};
use crate::domain::{now_ms, uid, Environment, Project, ResourceNode, ResourceNodeType};
use crate::error::AppError;

// =========================================================================
// 策略
// =========================================================================

/// 找到同名项时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfExists {
    /// 复用已有的那一个。这是默认值。
    ///
    /// 默认取这一档的理由：重复调用同一个「建」工具几乎总是意图相同的一件事，
    /// 而「建出两个同名模块」是一个没人会主动要求的结果。
    Reuse,
    /// 报冲突，不新建。调用方明确说「我要一个新的」时用这一档。
    Error,
    /// 另建一个，与已有同名项并存。
    Duplicate,
}

impl IfExists {
    /// 从调用方给的名字解析。空串与缺省都当 [`IfExists::Reuse`]。
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s.trim() {
            "" | "reuse" => Ok(IfExists::Reuse),
            "error" => Ok(IfExists::Error),
            "duplicate" => Ok(IfExists::Duplicate),
            other => Err(AppError::Validation(format!(
                "未知的 if_exists 值 '{other}'；可取 reuse / error / duplicate"
            ))),
        }
    }

    /// 该策略的字符串形式，与 [`IfExists::parse`] 的取值一一对应。
    pub fn as_str(self) -> &'static str {
        match self {
            IfExists::Reuse => "reuse",
            IfExists::Error => "error",
            IfExists::Duplicate => "duplicate",
        }
    }
}

// =========================================================================
// 结果
// =========================================================================

/// 寻址的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// 命中的那个节点的 id。
    pub id: String,
    /// 本次调用**新建**的为 `true`，复用了已有项的为 `false`。
    pub created: bool,
}

impl Resolved {
    fn reused(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            created: false,
        }
    }

    fn created(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            created: true,
        }
    }
}

// =========================================================================
// 寻址
// =========================================================================

/// 按名找或建一个项目。
///
/// 项目的名字**不唯一**（两个同名项目是合法的），所以「按名找」在这里是「找出第一个
/// 同名的」而不是「唯一的那个」。这仍是有用的：agent 反复应用同一份规格时，靠它
/// 认出「我刚才建的那个项目」。
///
/// `description` 与 `color` 只在**新建**时写进去。复用已有项目时它们一个都不动 ——
/// 「复用」的意思就是「不写任何字段」，改了它就不再是复用了。
pub fn resolve_project(
    conn: &mut Connection,
    name: &str,
    description: Option<&str>,
    color: Option<&str>,
    if_exists: IfExists,
) -> Result<Resolved, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("项目名不能为空".into()));
    }

    let projects = db::projects::list(conn)?;
    let found = projects.iter().find(|p| p.name.trim() == name);

    if let Some(p) = found {
        match if_exists {
            IfExists::Reuse => return Ok(Resolved::reused(p.id.clone())),
            IfExists::Error => {
                return Err(AppError::Validation(format!(
                "已经有一个叫「{name}」的项目了；换一个名字，或改用 if_exists=duplicate 另建一个"
            )))
            }
            IfExists::Duplicate => {}
        }
    }

    let now = now_ms();
    let project = Project {
        id: mint_id("proj"),
        name: name.to_string(),
        description: description.map(|d| d.to_string()),
        color: color.unwrap_or("#3B82F6").to_string(),
        created_at: now,
        updated_at: now,
    };
    db::projects::create(conn, &project)?;
    Ok(Resolved::created(project.id))
}

/// 在项目下按名找或建一个环境。
///
/// 环境的**列表**是项目级事实，名字在同一项目里也不唯一（界面允许两个同名环境），
/// 所以「按名找」是「找出第一个同名的」。新环境一律 `is_active = false` —— 导入
/// 一个环境不等于切换当前环境，那是一次显式决定。
pub fn resolve_environment(
    conn: &mut Connection,
    project_id: &str,
    name: &str,
    if_exists: IfExists,
) -> Result<Resolved, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("环境名不能为空".into()));
    }

    let envs = db::environments::list_by_project(conn, project_id)?;
    let found = envs.iter().find(|e| e.name.trim() == name);

    if let Some(e) = found {
        match if_exists {
            IfExists::Reuse => return Ok(Resolved::reused(e.id.clone())),
            IfExists::Error => {
                return Err(AppError::Validation(format!(
                    "这个项目里已经有一个叫「{name}」的环境了；换一个名字，或改用 if_exists=duplicate 另建一个"
                )))
            }
            IfExists::Duplicate => {}
        }
    }

    let env = Environment {
        id: mint_id("env"),
        project_id: project_id.to_string(),
        name: name.to_string(),
        module_base_urls: std::collections::HashMap::new(),
        variables: std::collections::HashMap::new(),
        disabled_variables: Vec::new(),
        is_active: false,
    };
    db::environments::create(conn, &env)?;
    Ok(Resolved::created(env.id))
}

/// 在项目下按名找或建一个模块。
///
/// 模块只能待在项目顶层（`ResourceNodeType::may_under` 强制），故查找范围就是
/// 「该项目下的模块」——不需要额外限定层级。
pub fn resolve_module(
    conn: &mut Connection,
    project_id: &str,
    name: &str,
    if_exists: IfExists,
) -> Result<Resolved, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("模块名不能为空".into()));
    }

    let siblings = db::nodes::list_by_root_and_type(conn, project_id, ResourceNodeType::Module)?;
    let found = siblings.iter().find(|m| m.name.trim() == name);

    if let Some(m) = found {
        match if_exists {
            IfExists::Reuse => return Ok(Resolved::reused(m.id.clone())),
            IfExists::Error => {
                return Err(AppError::Validation(format!(
                    "项目里已经有一个叫「{name}」的模块；换一个名字，或改用 if_exists=duplicate 另建一个"
                )))
            }
            // `duplicate` 不在这里返回：它要落到下面的新建分支去。
            IfExists::Duplicate => {}
        }
    }

    let now = now_ms();
    let node = ResourceNode {
        id: mint_id("mod"),
        root_id: project_id.to_string(),
        // 模块的父级是项目本身，不是另一个节点。
        parent_id: None,
        kind: ResourceNodeType::Module,
        name: name.to_string(),
        description: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let created = db::nodes::create(conn, &node, None)?;
    Ok(Resolved::created(created.id))
}

/// 在某个父下按名找或建一个集合。
///
/// `parent_collection_id` 给出时，集合挂在那个集合下（嵌套）；不给时挂在
/// `module_id` 下（顶层集合）。两种父都被 `may_under` 允许，校验由
/// [`db::nodes::create`] 执行 —— 本模块不自己判一遍，那道检查会与它漂移。
pub fn resolve_collection(
    conn: &mut Connection,
    module_id: &str,
    parent_collection_id: Option<&str>,
    name: &str,
    if_exists: IfExists,
) -> Result<Resolved, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("集合名不能为空".into()));
    }

    let parent_id = parent_collection_id.unwrap_or(module_id);
    // `root_id` 由模块推出：模块的行上就带着它，一次索引读。
    let root_id = db::nodes::root_id_of(conn, module_id)?;

    let siblings = db::nodes::list_children(conn, parent_id, Some(ResourceNodeType::Collection))?;
    let found = siblings.iter().find(|c| c.name.trim() == name);

    if let Some(c) = found {
        match if_exists {
            IfExists::Reuse => return Ok(Resolved::reused(c.id.clone())),
            IfExists::Error => {
                return Err(AppError::Validation(format!(
                    "这个父下已经有一个叫「{name}」的集合；换一个名字，或改用 if_exists=duplicate 另建一个"
                )))
            }
            IfExists::Duplicate => {}
        }
    }

    let now = now_ms();
    let node = ResourceNode {
        id: mint_id("col"),
        root_id,
        parent_id: Some(parent_id.to_string()),
        kind: ResourceNodeType::Collection,
        name: name.to_string(),
        description: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let created = db::nodes::create(conn, &node, None)?;
    Ok(Resolved::created(created.id))
}

/// 按路径逐级找或建集合。路径相对模块，第一段是模块下的顶层集合。
///
/// 返回每一级的结果，顺序与路径一致，调用方据此知道哪几级是这次新建的。
/// 路径为空串或含空段时报错 —— 「订单//明细」这种写法是调用方的笔误，
/// 悄悄当成「订单/明细」会让它建到别的地方去。
pub fn resolve_collection_path(
    conn: &mut Connection,
    module_id: &str,
    path: &[String],
    if_exists: IfExists,
) -> Result<Vec<Resolved>, AppError> {
    if path.is_empty() {
        return Err(AppError::Validation("集合路径不能为空".into()));
    }
    let mut out = Vec::with_capacity(path.len());
    let mut parent: Option<String> = None;
    for segment in path {
        let r = resolve_collection(conn, module_id, parent.as_deref(), segment, if_exists)?;
        parent = Some(r.id.clone());
        out.push(r);
    }
    Ok(out)
}

/// 把 `订单/明细` 这样的路径切成段。段内的空白被去掉，空段报错。
pub fn split_path(path: &str) -> Result<Vec<String>, AppError> {
    let mut out = Vec::new();
    for raw in path.split('/') {
        let seg = raw.trim();
        if seg.is_empty() {
            return Err(AppError::Validation(format!(
                "集合路径 '{path}' 里有空的一段；段与段之间只用一个斜杠"
            )));
        }
        out.push(seg.to_string());
    }
    if out.is_empty() {
        return Err(AppError::Validation("集合路径不能为空".into()));
    }
    Ok(out)
}

/// 在同一个父下按「名字 + method + url」找端点。找到返回它，找不到返回 `None`。
///
/// 父可以是集合，也可以是模块（端点允许直接挂在模块下）。
///
/// **名字参与判据**，这不是多余的严格：同一个集合下两个都叫「查询」的接口是常事，
/// 它们靠 method 与 url 区分；而两个同名同 method 同 url 的端点才是同一个接口被
/// 建了两遍。只按 method + url 判会把前者也认成同一个，于是第二个「查询」永远建不出来。
///
/// url 里的占位符不参与比较，故 `/users/{id}` 与 `/users/1` 是两个不同的端点，
/// 而 `/users/{id}` 与 `/users/{name}` 也是 —— 它们发出去的请求形状确实不同。
///
/// method 走 [`crate::domain::parse_http_method`] 归一，所以 `"post"` 与 `"POST"`
/// 比的是同一个动词 —— 判据里不留大小写这条岔路。
pub fn find_endpoint_by_identity(
    conn: &mut Connection,
    parent_id: &str,
    name: &str,
    method: &str,
    url: &str,
) -> Result<Option<String>, AppError> {
    let want = crate::domain::parse_http_method(method.trim())?;
    let children = db::nodes::list_children(conn, parent_id, Some(ResourceNodeType::Endpoint))?;
    for child in children {
        let detail = match db::endpoint_details::get(conn, &child.id) {
            Ok(d) => d,
            // 端点节点没有配置行是数据坏了，不该让整次寻址失败；跳过它，
            // 剩下的照常比。
            Err(_) => continue,
        };
        if child.name.trim() == name.trim()
            && detail.method == want
            && detail.url.trim() == url.trim()
        {
            return Ok(Some(child.id));
        }
    }
    Ok(None)
}

/// 铸一个带前缀的 id。
///
/// **绝不能用「前缀 + 毫秒时钟」**：那是毫秒时钟，同一毫秒内两次创建会得到
/// 同一个主键，第二次直接撞 `nodes.id` 约束。AI 客户端成批循环创建时必然踩到。
/// [`crate::domain::uid`] 是本仓库的 id 唯一来源：纳秒 + 进程内计数器，进程内不撞、
/// 重启也不重复前缀。
fn mint_id(prefix: &str) -> String {
    uid().replacen("sp_", &format!("{prefix}_"), 1)
}

// =========================================================================
// 测试
// =========================================================================

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
