//! `ir → db`：补 id、校验放置、指纹去重，全程一个事务。
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

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{now_ms, uid, EndpointDetail, ResourceNode, ResourceNodeType};
use simple_post_core::error::AppError;

use crate::diagnostics::{Diagnostic, Severity};
use crate::fingerprint::{FingerprintSet, RequestIdentity};
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule};

/// 一次落库的结果。调用方据此回报「新增几个、跳过几个、丢了什么」。
#[derive(Debug, Clone, Default)]
pub struct CommitReport {
    /// 新建的 endpoint 节点 id，按写入顺序。
    pub created_endpoint_ids: Vec<String>,
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

/// 一件还没写的事。显式工作栈而非递归：IR 的 collection 嵌套深度无上限，
/// 递归会让一棵畸形外来文件能靠栈深度把写路径带走。
enum Pending<'a> {
    Module {
        ir: &'a IrModule,
        sort: i64,
    },
    Collection {
        ir: &'a IrCollection,
        parent_id: String,
        sort: i64,
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
/// 全程一个 `BEGIN IMMEDIATE` 事务；中途任何一条被拒，整棵都不落。
pub fn commit(pool: &DbPool, project_id: &str, ex: &Exchange) -> Result<CommitReport, AppError> {
    db::with_tx(pool, |conn| {
        let mut report = CommitReport {
            diagnostics: ex.diagnostics.clone(),
            ..Default::default()
        };

        // 目标项目必须存在。`validate_placement` 也会查，但那次查以 `NotFound` 报一个
        // 「project」的错，而这里的调用方传的是一个自己选的落点。
        db::projects::get(conn, project_id)?;

        // 去重账本先灌入**已落库**的 endpoint，故「同一份 IR 落两次」第二次只跳过不新增。
        let existing = db::endpoint_details::list_by_root(conn, project_id)?;
        let mut fingerprints = FingerprintSet::new();
        fingerprints.absorb_details(existing.iter());

        let now = now_ms();
        let mut stack: Vec<Pending<'_>> = Vec::new();
        // 逆序入栈 → 出栈顺序与 IR 声明顺序一致，行号（sort_order）因此可预期。
        for (i, m) in ex.modules.iter().enumerate().rev() {
            stack.push(Pending::Module {
                ir: m,
                sort: i as i64,
            });
        }

        while let Some(item) = stack.pop() {
            match item {
                Pending::Module { ir, sort } => {
                    // 模块的父是项目本身。`nodes` 里没有项目行，故顶层模块的父存为 NULL，
                    // 由 `root_id` 说明它归哪个项目所有。
                    let node = ResourceNode {
                        id: uid(),
                        root_id: project_id.to_string(),
                        parent_id: None,
                        kind: ResourceNodeType::Module,
                        name: ir.name.clone(),
                        description: ir.description.clone(),
                        sort_order: sort,
                        created_at: now,
                        updated_at: now,
                    };
                    let module_id = db::nodes::create(conn, &node, None)?.id;

                    // 先压 collection 再压 endpoint：栈是后进先出，故这样出栈顺序才是
                    // 「先本层的 endpoint，再各 collection」，与 IR 的声明顺序一致。
                    // 树的显示顺序由 `sort_order` 决定，但报告里的 id 顺序也该可预期。
                    for (i, c) in ir.collections.iter().enumerate().rev() {
                        stack.push(Pending::Collection {
                            ir: c,
                            parent_id: module_id.clone(),
                            sort: i as i64,
                        });
                    }
                    for (i, ep) in ir.endpoints.iter().enumerate().rev() {
                        stack.push(Pending::Endpoint {
                            ir: ep,
                            parent_id: module_id.clone(),
                            sort: i as i64,
                        });
                    }
                }
                Pending::Collection {
                    ir,
                    parent_id,
                    sort,
                } => {
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

                    for (i, c) in ir.collections.iter().enumerate().rev() {
                        stack.push(Pending::Collection {
                            ir: c,
                            parent_id: collection_id.clone(),
                            sort: i as i64,
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
                    // 指纹认得出就跳过。记一笔 Info，好让「为什么少了几个」有出处。
                    if fingerprints.seen(&RequestIdentity::of_ir(ir)) {
                        report.skipped_duplicates.push(ir.name.clone());
                        report.diagnostics.push(
                            Diagnostic::info(
                                format!("/{}", ir.path),
                                "项目里已有同一个接口，按指纹跳过",
                            )
                            .dropping(format!(
                                "{} {}",
                                ir.method.as_str(),
                                ir.path
                            )),
                        );
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

// 测试——结构、去重与事务边界。

#[cfg(test)]
#[path = "commit_test.rs"]
mod tests;
