//! `ir → db`：补 id、校验放置、按选择集裁剪、指纹去重，全程一个事务。
//!
//! 住在本 crate 而不是 Tauri command 里，是刻意的：AGENTS.md 的 dual-transport invariant
//! 说的就是逻辑泄漏到传输层的代价。落库逻辑只有一份，Tauri command 与 REST route
//! 都退化成薄壳。
//!
//! **整个过程在一个 `db::with_tx` 里**：一棵含多层嵌套的 IR 要么整棵进去，要么一行不进。
//! 半棵树比没有树更难读——用户看到的是一棵能展开、点开却报「接口已删除」的树。
//!
//! 落库的每一行走 [`db::nodes::create`]，故父子矩阵由 `validate_placement` 强制，
//! 本模块不自己再判一遍：那道检查已经在那里，再写一份只会与它不一致。

mod base_url;
mod dedupe;

use std::collections::BTreeSet;

use simple_post_core::db::{self, Connection, DbPool};
use simple_post_core::domain::{
    now_ms, uid, EndpointDetail, NodeScriptsPatch, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;

use crate::diagnostics::{Diagnostic, Severity};
use crate::fingerprint::FingerprintSet;
use crate::ir::{Exchange, IrBaseUrl, IrCollection, IrEndpoint, IrModule, IrScripts};

use self::base_url::wire_base_url;
use self::dedupe::{decide, has_any, has_any_collection};
// 预览经 `simple_post_interchange::commit::plan_base_url` 取这份计划，故 `base_url` 是私有
// 模块，而 `plan_base_url` / `BaseUrlPlan` 要在这里转出去。
pub use self::base_url::{plan_base_url, BaseUrlPlan};

/// 一次落库的结果。调用方据此回报「新增几个、跳过几个、丢了什么」。
#[derive(Debug, Clone, Default)]
pub struct CommitReport {
    /// 新建的 endpoint 节点 id，按写入顺序。
    pub created_endpoint_ids: Vec<String>,
    /// 新建的集合节点 id，按写入顺序。展开新建的分组要用它——渲染层的 `expanded`
    /// 不落库，故前端拿不到「哪些组是刚建的」就只能全不展开。
    pub created_collection_ids: Vec<String>,
    /// 被指纹认出来而跳过的条目。跳过不是丢弃：它们仍在 IR 里，且带了一条
    /// [`Severity::Info`] 诊断说明为什么没写。
    pub skipped_duplicates: Vec<String>,
    /// IR 自带的诊断加上本次落库新增的诊断，一并带回。预览与落库是两次调用，
    /// 各自持有自己的那份，外不该假设它们是同一个对象。
    pub diagnostics: Vec<Diagnostic>,
    /// 顺带落下的地址，按写入顺序。为空表示这一次没有地址可落：文档没给 host，
    /// 或调用方关掉了它。
    pub base_urls: Vec<AppliedBaseUrl>,
}

/// 一次导入落下的一格地址。用户要知道「地址去了哪」——否则他打开目标模块看到的是
/// 一排 `{{baseUrl}}` 形式的路径，却没有任何地方写着 baseUrl 是多少。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedBaseUrl {
    /// 写进那一格的后端 api 地址。
    pub url: String,
    /// 写进那一格的实际访问地址。外来文档给不出代理，故导入时恒为 `None`。
    pub proxy: Option<String>,
    /// 写进了哪个模块的格子。
    pub module_id: String,
    /// 写进了哪个环境。项目没有激活环境时为 `None`——地址没有落进任何一格。
    pub environment_name: Option<String>,
}

impl CommitReport {
    /// 值得让用户看的那几条——[`Severity::Info`] 是「按设计跳过」，不是问题。
    pub fn notable_diagnostics(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .collect()
    }
}

/// IR 顶层那些「模块」往哪儿落。
#[derive(Debug, Clone, Copy)]
pub enum CommitTarget<'a> {
    /// 各自新建一个顶层模块。这是资源树里模块唯一的合法位置。
    ProjectRoot,
    /// 挂进一个已存在的模块。**此时 IR 顶层模块一律降级成集合。**
    ///
    /// 这不是取巧，是 [`ResourceNodeType::may_under`] 强制的：模块只能待在项目顶层，
    /// 模块下挂模块会被 SQLite 触发器与应用层检查双重拒绝。故「把外来文件导进某个
    /// 已有的模块下」在树契约里唯一的表达就是「降一级落成集合」。
    Module { module_id: &'a str },
}

/// 一件还没写的事。显式工作栈而非递归：IR 的 collection 嵌套深度无上限，
/// 递归会让一棵畸形外来文件能靠栈深度把写路径带走。
enum Pending<'a> {
    /// 顶层分组。`anchor` 为 `None` 时落成 Module，为 `Some(父)` 时落成 Collection。
    Group {
        ir: &'a IrModule,
        sort: i64,
        anchor: Option<String>,
        /// 这棵子树里的端点继承哪一格。落到项目顶层时是本模块自己那一格；挂进已有
        /// 模块时是锚点模块那一格——顶层模块降级成了集合，而集合没有格子。
        inherited_cell_url: Option<String>,
        /// 这棵子树里还有没有**会被写入**的端点。没有就不写——空的分组在树里
        /// 只会让人以为导入了什么。注意判据是 `writable` 而不是 `selected`：
        /// 全勾上却全被指纹去重，同样等于什么都没写。
        keep: bool,
    },
    Collection {
        ir: &'a IrCollection,
        parent_id: String,
        sort: i64,
        /// 同 [`Pending::Group::inherited_cell_url`]：集合自己没有格子，继承上方那一格。
        inherited_cell_url: Option<String>,
        /// 同 [`Pending::Group::keep`]。
        keep: bool,
    },
    Endpoint {
        ir: &'a IrEndpoint,
        parent_id: String,
        sort: i64,
        /// 同 [`Pending::Group::inherited_cell_url`]。
        inherited_cell_url: Option<String>,
    },
}

/// 把一份 IR 落进 `project_id` 指名的项目。
///
/// `project_id` 是调用方选定的落点——IR 自身没有项目 id，因为项目不是节点，
/// 它是由 `root_id` 指名的隐式根。
///
/// `selected` 是端点 [`IrEndpoint::key`] 的选择集。**未勾选的端点整条不写，且不发诊断**：
/// 用户主动取消勾选不是问题，报一条「跳过」只会把真正该看的诊断淹没。传
/// [`Exchange::endpoint_keys`] 即全选。
///
/// 全程一个 `BEGIN IMMEDIATE` 事务；中途任何一条被拒，整棵都不落。
///
/// `apply_base_url` 为假时完全不碰地址：文档里的 host 就当没看见，端点也不带自己的
/// 地址，与本次导入之前的行为逐字节相同。
pub fn commit(
    pool: &DbPool,
    project_id: &str,
    target: CommitTarget<'_>,
    ex: &Exchange,
    selected: &BTreeSet<String>,
    apply_base_url: bool,
) -> Result<CommitReport, AppError> {
    db::with_tx(pool, |conn| {
        commit_in_tx(conn, project_id, target, ex, selected, apply_base_url)
    })
}

/// [`commit`] 的事务内版本。调用方**自己**开事务。
///
/// 存在的理由：一次导入要写的不止树——Postman 的环境是项目级的另一张表，它必须与树
/// 同生共死。而 [`db::with_tx`] 的闭包拿不到另一个连接（`BEGIN IMMEDIATE` 会与外层
/// 那个自己死锁），所以编排层拿到的是这一个「已持有连接」的版本。
pub fn commit_in_tx(
    conn: &mut Connection,
    project_id: &str,
    target: CommitTarget<'_>,
    ex: &Exchange,
    selected: &BTreeSet<String>,
    apply_base_url: bool,
) -> Result<CommitReport, AppError> {
    let mut report = CommitReport {
        diagnostics: ex.diagnostics.clone(),
        ..Default::default()
    };

    // 目标项目必须存在。`validate_placement` 也会查，但那次查以 `NotFound` 报一个
    // 「project」的错，而这里的调用方传的是一个自己选的落点。
    db::projects::get(conn, project_id)?;

    // 落进已有模块时，先把那个锚点验一遍：它可能是一个已经被删掉的 id，
    // 也可能是一个集合（界面上换了个入口就有可能点到）。两种情况都得报得出。
    let anchor_id = match target {
        CommitTarget::ProjectRoot => None,
        CommitTarget::Module { module_id } => {
            let kind = db::nodes::type_of(conn, module_id)?;
            if kind != ResourceNodeType::Module {
                return Err(AppError::Validation(format!(
                    "导入落点必须是模块，而 {module_id} 是一个 {}",
                    kind.as_str()
                )));
            }
            if db::nodes::root_id_of(conn, module_id)? != project_id {
                return Err(AppError::Validation("导入落点不属于该项目".into()));
            }
            Some(module_id.to_string())
        }
    };

    // 地址住进 (环境 × 模块) 那一格。环境的答案是唯一的：项目当前激活的那一个。
    // 没有激活环境就没有格子可填，地址便无处可去——报告里说得清楚，用户能看见
    // 「地址没有落进任何一格」。
    let plan = if apply_base_url {
        let plan = plan_base_url(conn, project_id, ex)?;
        if let Some(d) = plan.multiple_diagnostic() {
            report.diagnostics.push(d);
        }
        Some(plan)
    } else {
        None
    };
    let environment = plan.as_ref().and_then(|p| p.environment.as_ref());

    // 挂进已有模块时，那一格归锚点模块吃下：整份文档的第一个地址。文档里其余的地址
    // 由各端点自己带着——端点带自己的地址时就不套用这一格。
    //
    // 落到项目顶层时每个模块吃下自己那一格：模块就是下面几行刚建的那些，格子跟着
    // 模块走，不必让用户逐个去填。
    let anchor_cell: Option<IrBaseUrl> = ex.modules.iter().find_map(|m| m.base_url.clone());

    // 挂进已有模块时，顶层模块一律降级成集合，而集合没有格子：那一格归锚点模块，
    // 吃下整份文档的第一个地址。文档里其余的地址由各端点自己带着。
    if let (Some(module_id), Some(cell)) = (&anchor_id, &anchor_cell) {
        if apply_base_url {
            wire_base_url(
                conn,
                module_id,
                &cell.url,
                cell.proxy.as_deref(),
                environment,
                &mut report,
            )?;
        }
    }

    // 去重账本先灌入**已落库**的 endpoint，故「同一份 IR 落两次」第二次只跳过不新增。
    let existing = db::endpoint_details::list_by_root(conn, project_id)?;
    let mut fingerprints = FingerprintSet::new();
    fingerprints.absorb_details(existing.iter());

    // 第一遍：在**一份克隆**上判定每个端点的下场，据此决定哪些容器值得建。
    //
    // 只按 `selected` 判空是不够的——用户可能把一整份文件都勾上了，却因为它们
    // 全都在项目里已经有了而被指纹全部跳过。那种情况下按 `selected` 判出来的
    // 每个容器都「有人」，于是树里落下一排点开什么都没有的集合。故判据是
    // 「会被写入」。
    //
    // 去重是**有状态**的（[`FingerprintSet::seen`] 记账，故同一份 IR 里重复粘贴
    // 的条目只有第一个会写），所以判定只能走一遍：写在第二遍再判一次，同一个
    // 端点会得到两个不同答案，而第二个答案才是错的。
    //
    // 「跳过了几个」也在这里出，不在写入遍：全被去重时一个容器都不建，写入遍
    // 压根不会走到那些端点，跳过记录会跟着一起消失，用户就看到「新增 0、
    // 跳过 0」而不知道东西去哪了。
    let fate = decide(&ex.modules, &mut fingerprints.clone(), selected);
    let writable = &fate.writable;

    for dup in &fate.duplicates {
        report.skipped_duplicates.push(dup.name.clone());
        report.diagnostics.push(
            // 指针是端点在 IR 里的 key（导入时形如 `/item/3/request`，导出时是 `nodes.id`），
            // 不是它的 path：path 是地址，在源文件里未必指得到任何一行，而渲染层把
            // pointer 原样显示给用户——一个指向不存在位置的指针等于没给。
            Diagnostic::info(dup.key.clone(), "项目里已有同一个接口，按指纹跳过")
                .dropping(format!("{} {}", dup.method, dup.path)),
        );
    }

    let now = now_ms();
    let mut written: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<Pending<'_>> = Vec::new();
    // 逆序入栈 → 出栈顺序与 IR 声明顺序一致，行号（sort_order）因此可预期。
    for (i, m) in ex.modules.iter().enumerate().rev() {
        // 落到项目顶层时这一格是本模块自己的；挂进已有模块时是锚点模块那一格。
        let inherited_cell_url = match &anchor_id {
            Some(_) => anchor_cell.as_ref().map(|c| c.url.clone()),
            None => m.base_url.as_ref().map(|c| c.url.clone()),
        };
        stack.push(Pending::Group {
            ir: m,
            sort: i as i64,
            anchor: anchor_id.clone(),
            inherited_cell_url,
            keep: has_any(m, writable),
        });
    }

    while let Some(item) = stack.pop() {
        match item {
            Pending::Group {
                ir,
                sort,
                anchor,
                inherited_cell_url,
                keep,
            } => {
                if !keep {
                    continue;
                }
                // 模块的父是项目本身。`nodes` 里没有项目行，故顶层模块的父存为 NULL，
                // 由 `root_id` 说明它归哪个项目所有。有锚点时它是集合，父就是锚点。
                let as_module = anchor.is_none();
                let node = ResourceNode {
                    id: uid(),
                    root_id: project_id.to_string(),
                    parent_id: anchor.clone(),
                    kind: if as_module {
                        ResourceNodeType::Module
                    } else {
                        ResourceNodeType::Collection
                    },
                    name: ir.name.clone(),
                    description: ir.description.clone(),
                    sort_order: sort,
                    created_at: now,
                    updated_at: now,
                };
                let group_id = db::nodes::create(conn, &node, None)?.id;
                if !as_module {
                    report.created_collection_ids.push(group_id.clone());
                } else if apply_base_url {
                    if let Some(cell) = &ir.base_url {
                        // 落到项目顶层时这一格归新建的模块自己。写的是后端 api 地址与
                        // 实际访问地址这一对；外来文档给得出地址，给不出代理。
                        wire_base_url(
                            conn,
                            &group_id,
                            &cell.url,
                            cell.proxy.as_deref(),
                            environment,
                            &mut report,
                        )?;
                    }
                }
                // 模块**不能**存脚本（`node_scripts` 只认 collection）。落进已有模块时
                // 这一层降级成了集合，脚本就有归宿了；落到项目顶层时它只能是模块，
                // 脚本无处可去——说出来，不静默丢。
                write_scripts(
                    conn,
                    &group_id,
                    as_module,
                    &ir.scripts,
                    &ir.key,
                    &mut report.diagnostics,
                )?;

                // 先压 collection 再压 endpoint：栈是后进先出，故这样出栈顺序才是
                // 「先本层的 endpoint，再各 collection」，与 IR 的声明顺序一致。
                // 树的显示顺序由 `sort_order` 决定，但报告里的 id 顺序也该可预期。
                for (i, c) in ir.collections.iter().enumerate().rev() {
                    stack.push(Pending::Collection {
                        ir: c,
                        parent_id: group_id.clone(),
                        sort: i as i64,
                        inherited_cell_url: inherited_cell_url.clone(),
                        keep: has_any_collection(c, writable),
                    });
                }
                for (i, ep) in ir.endpoints.iter().enumerate().rev() {
                    stack.push(Pending::Endpoint {
                        ir: ep,
                        parent_id: group_id.clone(),
                        sort: i as i64,
                        inherited_cell_url: inherited_cell_url.clone(),
                    });
                }
            }
            Pending::Collection {
                ir,
                parent_id,
                sort,
                inherited_cell_url,
                keep,
            } => {
                if !keep {
                    continue;
                }
                let node = ResourceNode {
                    id: uid(),
                    root_id: project_id.to_string(),
                    parent_id: Some(parent_id.clone()),
                    kind: ResourceNodeType::Collection,
                    name: ir.name.clone(),
                    description: ir.description.clone(),
                    sort_order: sort,
                    created_at: now,
                    updated_at: now,
                };
                let collection_id = db::nodes::create(conn, &node, None)?.id;
                report.created_collection_ids.push(collection_id.clone());
                // 集合能存脚本，故直接写。
                write_scripts(
                    conn,
                    &collection_id,
                    false,
                    &ir.scripts,
                    &ir.key,
                    &mut report.diagnostics,
                )?;

                for (i, c) in ir.collections.iter().enumerate().rev() {
                    stack.push(Pending::Collection {
                        ir: c,
                        parent_id: collection_id.clone(),
                        sort: i as i64,
                        inherited_cell_url: inherited_cell_url.clone(),
                        keep: has_any_collection(c, writable),
                    });
                }
                for (i, ep) in ir.endpoints.iter().enumerate().rev() {
                    stack.push(Pending::Endpoint {
                        ir: ep,
                        parent_id: collection_id.clone(),
                        sort: i as i64,
                        inherited_cell_url: inherited_cell_url.clone(),
                    });
                }
            }
            Pending::Endpoint {
                ir,
                parent_id,
                sort,
                inherited_cell_url,
            } => {
                // 没被判定为「会写」的（没勾选、或已被指纹认出）整条不写。
                // 两者的区别只体现在第一遍报出的跳过记录上，不落到树里。
                if !writable.contains(&ir.key) {
                    continue;
                }
                // 同一个 key 只写一次。`key` 按约定是「一个 IR 节点一个」，
                // 故这条在真实外来文件里永不触发——但它把「解析器给出重复 key」
                // 的失败形态从「写进去两份」变成「写一份」，而后者不会有人发现。
                if !written.insert(ir.key.clone()) {
                    continue;
                }

                let node = ResourceNode {
                    id: uid(),
                    root_id: project_id.to_string(),
                    parent_id: Some(parent_id),
                    kind: ResourceNodeType::Endpoint,
                    name: ir.name.clone(),
                    description: ir.description.clone(),
                    sort_order: sort,
                    created_at: now,
                    updated_at: now,
                };
                let detail = EndpointDetail {
                    node_id: node.id.clone(),
                    method: ir.method.clone(),
                    // `ir` 存的是 path-only，`domain` 这一列同样是 path，故这是一次改名。
                    url: ir.path.clone(),
                    // 环境取自上面那一次 `plan_base_url`：地址住进 (环境 × 模块) 那一格，
                    // 而格子只在某一个环境里。端点带上那个环境，解析时才找得到那一格。
                    environment_id: environment.map(|e| e.id.clone()),
                    // 端点自己的地址。与继承来的那一格相同时不写——那表示「跟随模块」，
                    // 用户改了模块的地址时这个端点也跟着走。地址不同时才写自己的。
                    base_url_override: if apply_base_url {
                        endpoint_override(ir, inherited_cell_url.as_deref())
                    } else {
                        None
                    },
                    params: ir.params.clone(),
                    path_params: ir.path_params.clone(),
                    headers: ir.headers.clone(),
                    body: ir.body.clone(),
                    auth: ir.auth.clone(),
                    docs: ir.docs.clone(),
                    notes: ir.notes.clone(),
                    tags: ir.tags.clone(),
                    // 端点的脚本住在 `endpoint_details`，与 body / headers 同级。
                    pre_request_script: ir.scripts.pre_request_script.clone(),
                    test_script: ir.scripts.test_script.clone(),
                    // 导入进来的接口不带 HTTPS 出站设置：证书路径是本机的东西，导到另一台
                    // 机器上没有意义。故导入的端点一律继承全局默认。
                    tls: None,
                };

                db::nodes::create(conn, &node, Some(&detail))?;
                report.created_endpoint_ids.push(node.id);
            }
        }
    }

    Ok(report)
}

/// 端点自己的地址。
///
/// 与继承来的那一格相同 → 不写，那表示「跟随模块」，用户改了模块的地址时这个端点也跟着
/// 走；不同 → 写自己的那一份。端点本来就没有自己的地址 → 不写，它继承模块那一格。
fn endpoint_override(ep: &IrEndpoint, inherited_cell_url: Option<&str>) -> Option<String> {
    let own = ep.base_url_override.clone()?;
    match inherited_cell_url {
        Some(cell) if cell == own => None,
        _ => Some(own),
    }
}

/// 把一个节点上的两段脚本写进 `node_scripts`。
///
/// `as_module` 为真表示这个节点落成了**模块**——而模块不能存脚本（`node_scripts`
/// 只认 collection，应用层与 SQLite 触发器各查一次）。那种情况下脚本无处可去，
/// 出诊断说明，不静默丢。
fn write_scripts(
    conn: &mut Connection,
    node_id: &str,
    as_module: bool,
    scripts: &IrScripts,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), AppError> {
    if scripts.is_empty() {
        return Ok(());
    }
    if as_module {
        diagnostics.push(Diagnostic::warn(
            // 指针是端点在 IR 里的 key，即源文件里的位置指针（导入时形如
            // `/item/3`）——那才是用户能对着源文件找到的地方。
            key.to_string(),
            "这个分组落成了模块，而模块不能存脚本，这两段脚本没有导入",
        ));
        return Ok(());
    }
    db::node_scripts::set(
        conn,
        node_id,
        NodeScriptsPatch {
            pre_request_script: scripts.pre_request_script.clone().map(Some),
            test_script: scripts.test_script.clone().map(Some),
        },
    )?;
    Ok(())
}

/// 「整份文档」这一事实的 pointer。多个 host 时没有单一位置能指过去——它们散在文件
/// 各处，指任何一个都是撒谎，故指根。
const DOC_ROOT: &str = "/";

// 测试——结构与事务边界、去重、选择集、地址。

#[cfg(test)]
mod support;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_tests;

#[cfg(test)]
#[path = "dedupe_test.rs"]
mod dedupe_tests;

#[cfg(test)]
#[path = "selection_test.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "base_url_test.rs"]
mod base_url_tests;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_tests;
