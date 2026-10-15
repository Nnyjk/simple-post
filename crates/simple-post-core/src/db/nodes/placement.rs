//! 写入前的两道前置检查：放置是否合法，以及节点与 detail 行是否配套。

use rusqlite::Connection;

use crate::domain::{EndpointDetail, ResourceNode, ResourceNodeType};
use crate::error::AppError;

use super::read::get;

/// 写入前先检查一次放置是否合法。四种可能出错的地方：
/// * 挂在一个类型不接受它的父下
/// * 模块被给了节点父
/// * `root_id` 与父的不一致——那会让节点归档在一个项目下、却渲染在另一个项目的树里
/// * 顶层节点没有指名项目，或指名的项目不存在
///
/// 最后一条尤其要紧：两条传输层都把缺席的 `rootId` 默认成空串，而顶层节点没有 `parentId`
/// 可供推导，空串于是直达 `INSERT`，SQLite 报 `FOREIGN KEY constraint failed`——那会以
/// 500 加 `code: "sqlite"` 出现，把一个调用方的错误说成传输故障，而错误封装无法让客户端
/// 把它与真正的服务端故障区分开。
pub(super) fn validate_placement(
    conn: &mut Connection,
    kind: ResourceNodeType,
    parent_id: Option<&str>,
    root_id: &str,
) -> Result<(), AppError> {
    let Some(parent_id) = parent_id else {
        if kind != ResourceNodeType::Module {
            return Err(AppError::Validation(format!(
                "a {} needs a parent node; only a module can sit at the top of a project",
                kind.as_str()
            )));
        }
        // 模块的父是项目本身，行无法引用它——所以项目必须以某种方式被指名。
        if root_id.is_empty() {
            return Err(AppError::Validation(
                "a top-level node needs its project: pass rootId, or a parentId to derive it from"
                    .into(),
            ));
        }
        if crate::db::projects::get(conn, root_id).is_err() {
            return Err(AppError::Validation(format!(
                "no project with id {root_id:?}; a top-level node must name the project it belongs to"
            )));
        }
        return Ok(());
    };

    let parent = get(conn, parent_id)?;
    if !kind.may_under(parent.kind) {
        return Err(AppError::Validation(format!(
            "a {} cannot sit under a {}",
            kind.as_str(),
            parent.kind.as_str()
        )));
    }
    if parent.root_id != root_id {
        return Err(AppError::Validation(format!(
            "node belongs to project {} but its parent belongs to {}",
            root_id, parent.root_id
        )));
    }
    Ok(())
}

/// 一个 endpoint 恰有一行 detail；module 或 collection 不得有。
/// 前置检查，好让忘传——或给 collection 传了 detail 的——调用方拿到一条指名错误的校验错误，
/// 而不是一棵打开该条目时报告缺行的树。
pub(super) fn check_detail_agreement(
    node: &ResourceNode,
    detail: Option<&EndpointDetail>,
) -> Result<(), AppError> {
    match (node.kind, detail) {
        // 顺序要紧：合法的 endpoint 分支必须先于下面那条「不允许 detail」的分支匹配，否则被它吞掉。
        (ResourceNodeType::Endpoint, Some(d)) if d.node_id == node.id => Ok(()),
        (ResourceNodeType::Endpoint, Some(EndpointDetail { node_id, .. })) => {
            Err(AppError::Validation(format!(
                "endpoint detail is for node {node_id} but the node is {}",
                node.id
            )))
        }
        (ResourceNodeType::Endpoint, None) => Err(AppError::Validation(format!(
            "endpoint node {} needs an endpoint detail payload",
            node.id
        ))),
        (_, Some(_)) => Err(AppError::Validation(format!(
            "a {} node takes no endpoint detail payload",
            node.kind.as_str()
        ))),
        (_, None) => Ok(()),
    }
}
