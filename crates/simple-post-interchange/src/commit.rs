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
//! 本模块不再自己判一遍：那道检查已经在那里，再写一份只会与它漂移。

use std::collections::BTreeSet;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{now_ms, uid, EndpointDetail, ResourceNode, ResourceNodeType};
use simple_post_core::error::AppError;

use crate::diagnostics::{Diagnostic, Severity};
use crate::fingerprint::{FingerprintSet, RequestIdentity};
use crate::ir::{endpoint_keys_of, Exchange, IrCollection, IrEndpoint, IrModule};

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
        /// 这棵子树里还有没有**会被写入**的端点。没有就不写——空的分组在树里
        /// 只会让人以为导入了什么。注意判据是 `writable` 而不是 `selected`：
        /// 全勾上却全被指纹去重，同样等于什么都没写。
        keep: bool,
    },
    Collection {
        ir: &'a IrCollection,
        parent_id: String,
        sort: i64,
        /// 同 [`Pending::Group::keep`]。
        keep: bool,
    },
    Endpoint {
        ir: &'a IrEndpoint,
        parent_id: String,
        sort: i64,
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
pub fn commit(
    pool: &DbPool,
    project_id: &str,
    target: CommitTarget<'_>,
    ex: &Exchange,
    selected: &BTreeSet<String>,
) -> Result<CommitReport, AppError> {
    db::with_tx(pool, |conn| {
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
                Diagnostic::info(format!("/{}", dup.path), "项目里已有同一个接口，按指纹跳过")
                    .dropping(format!("{} {}", dup.method, dup.path)),
            );
        }

        let now = now_ms();
        let mut written: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<Pending<'_>> = Vec::new();
        // 逆序入栈 → 出栈顺序与 IR 声明顺序一致，行号（sort_order）因此可预期。
        for (i, m) in ex.modules.iter().enumerate().rev() {
            stack.push(Pending::Group {
                ir: m,
                sort: i as i64,
                anchor: anchor_id.clone(),
                keep: has_any(m, writable),
            });
        }

        while let Some(item) = stack.pop() {
            match item {
                Pending::Group {
                    ir,
                    sort,
                    anchor,
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
                    }

                    // 先压 collection 再压 endpoint：栈是后进先出，故这样出栈顺序才是
                    // 「先本层的 endpoint，再各 collection」，与 IR 的声明顺序一致。
                    // 树的显示顺序由 `sort_order` 决定，但报告里的 id 顺序也该可预期。
                    for (i, c) in ir.collections.iter().enumerate().rev() {
                        stack.push(Pending::Collection {
                            ir: c,
                            parent_id: group_id.clone(),
                            sort: i as i64,
                            keep: has_any_collection(c, writable),
                        });
                    }
                    for (i, ep) in ir.endpoints.iter().enumerate().rev() {
                        stack.push(Pending::Endpoint {
                            ir: ep,
                            parent_id: group_id.clone(),
                            sort: i as i64,
                        });
                    }
                }
                Pending::Collection {
                    ir,
                    parent_id,
                    sort,
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

                    for (i, c) in ir.collections.iter().enumerate().rev() {
                        stack.push(Pending::Collection {
                            ir: c,
                            parent_id: collection_id.clone(),
                            sort: i as i64,
                            keep: has_any_collection(c, writable),
                        });
                    }
                    for (i, ep) in ir.endpoints.iter().enumerate().rev() {
                        stack.push(Pending::Endpoint {
                            ir: ep,
                            parent_id: collection_id.clone(),
                            sort: i as i64,
                        });
                    }
                }
                Pending::Endpoint {
                    ir,
                    parent_id,
                    sort,
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
                        environment_id: None,
                        // 服务与环境是本仓库的概念，外来格式给不出。写 `None` 的含义是
                        // 「用项目的 ★」，那是一个**真实答案**，不是「还没定」——故不猜。
                        base_url_definition_id: None,
                        base_url_override: None,
                        params: ir.params.clone(),
                        headers: ir.headers.clone(),
                        body: ir.body.clone(),
                        auth: ir.auth.clone(),
                        docs: ir.docs.clone(),
                        notes: ir.notes.clone(),
                        tags: ir.tags.clone(),
                    };

                    db::nodes::create(conn, &node, Some(&detail))?;
                    report.created_endpoint_ids.push(node.id);
                }
            }
        }

        Ok(report)
    })
}

/// 一个被指纹认出来而跳过的请求。名字与请求描述给报告用。
struct SkippedDuplicate {
    name: String,
    method: String,
    path: String,
}

/// 每个端点的下场：会被写入，还是已被认出而跳过。
struct Fate {
    writable: BTreeSet<String>,
    duplicates: Vec<SkippedDuplicate>,
}

/// 按写入顺序走一遍，判定每个勾中的端点是写还是跳过。
///
/// `ledger` 是去重账本，本遍会往里记账，故调用方必须传一份克隆。顺序必须与真正落库
/// 一致（先本层 endpoint，再各 collection 依次深入），否则「哪些是重复」会有另一套答案。
fn decide(modules: &[IrModule], ledger: &mut FingerprintSet, selected: &BTreeSet<String>) -> Fate {
    let mut writable = BTreeSet::new();
    let mut duplicates = Vec::new();
    let mut claim = |ep: &IrEndpoint| {
        if !selected.contains(&ep.key) {
            return;
        }
        if ledger.seen(&RequestIdentity::of_ir(ep)) {
            duplicates.push(SkippedDuplicate {
                name: ep.name.clone(),
                method: ep.method.as_str().to_string(),
                path: ep.path.clone(),
            });
        } else {
            writable.insert(ep.key.clone());
        }
    };
    for m in modules {
        for ep in &m.endpoints {
            claim(ep);
        }
        let mut stack: Vec<&IrCollection> = m.collections.iter().rev().collect();
        while let Some(c) = stack.pop() {
            for ep in &c.endpoints {
                claim(ep);
            }
            stack.extend(c.collections.iter().rev());
        }
    }
    Fate {
        writable,
        duplicates,
    }
}

/// 这个分组里还有没有会被写入的端点。空分组不写。
fn has_any(m: &IrModule, writable: &BTreeSet<String>) -> bool {
    endpoint_keys_of(&m.endpoints, &m.collections)
        .iter()
        .any(|k| writable.contains(k))
}

fn has_any_collection(c: &IrCollection, writable: &BTreeSet<String>) -> bool {
    endpoint_keys_of(&c.endpoints, &c.collections)
        .iter()
        .any(|k| writable.contains(k))
}

// 测试——结构、去重、选择集与事务边界。

#[cfg(test)]
#[path = "commit_test.rs"]
mod tests;
