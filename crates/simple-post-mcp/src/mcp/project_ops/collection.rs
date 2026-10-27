//! `collection_*` MCP 工具 —— 集合 CRUD 与在模块内的移动，共 5 个。
//!
//! `collection_list` 复用 `super::ModuleIdArgs`；其余 Args 私有于本模块。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::{arg_str, AppState, CallToolResult};
use crate::tool_fn;
use simple_post_core::domain::{ResourceNodePatch, ResourceNodeType};
use simple_post_core::error::AppError;

/// 解析节点所属项目，供按项目的那扇门使用。
///
/// 绝不能写死字面量 "global"：它不是项目 id，会拿「无此行时的默认 `ReadCall`」
/// 去比写工具要求的 `Full`，于是每次调用都被拒，而工具看起来注册得好好的。
/// `root_id` 已反规范化在每一行上，正确答案就是一次索引读，不必回溯父链。
fn owning_project(pool: &simple_post_core::db::DbPool, node_id: &str) -> Result<String, AppError> {
    simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, node_id)
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct CollectionCreateArgs {
    pub module_id: String,
    #[serde(default)]
    pub parent_collection_id: Option<String>,
    /// 集合名。给了 `path` 就不用它。
    #[serde(default)]
    pub name: Option<String>,
    /// 相对模块的集合路径，段与段之间一个斜杠，如 `订单/明细`。路径上的每一级
    /// 缺了就建、有了就复用。
    #[serde(default)]
    pub path: Option<String>,
    /// 同一个父下已经有同名集合时怎么办。缺省是 `reuse`。
    #[serde(default)]
    pub if_exists: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CollectionUpdateArgs {
    pub collection_id: String,
    #[serde(default)]
    pub patch: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CollectionMoveArgs {
    pub collection_id: String,
    pub new_module_id: String,
    #[serde(default)]
    pub new_parent_collection_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // 留给将来的排序接口
    pub new_sort_order: Option<i64>,
}

pub(super) fn register(v: &mut Vec<crate::mcp::ToolEntry>) {
    v.push(crate::mcp::ToolEntry {
        name: "collection_list",
        description: "List collections in a module (top-level + nested).",
        schema: json!({
            "type": "object",
            "properties": { "module_id": { "type": "string" } },
            "required": ["module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_list),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_create",
        description: "Create a collection. Give it a `name` for one collection under \
                       the module (or under `parent_collection_id`), or a `path` like \
                       \"订单/明细\" to create the whole chain at once. Every level on the \
                       way is created when missing and reused when it already exists.",
        schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string" },
                "parent_collection_id": { "type": "string" },
                "name": { "type": "string", "description": "Name of the collection. Not needed when `path` is given." },
                "path": {
                    "type": "string",
                    "description": "Collection path relative to module_id, segments separated by '/', e.g. \"订单/明细\". Every level on the way is created when missing and reused when it already exists."
                },
                "if_exists": {
                    "type": "string",
                    "enum": ["reuse", "error", "duplicate"],
                    "description": "What to do when the target parent already has a collection with this name. \"reuse\" (default) returns the existing one; \"error\" refuses; \"duplicate\" creates a second one."
                }
            },
            "required": ["module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_create),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_update",
        description: "Patch a collection's name/description/parent.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "patch": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "parentCollectionId": { "type": ["string", "null"] }
                    },
                    "additionalProperties": true
                }
            },
            "required": ["collection_id", "patch"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_update),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_delete",
        description: "Delete a collection (and its nested collections + endpoints).",
        schema: json!({
            "type": "object",
            "properties": { "collection_id": { "type": "string" } },
            "required": ["collection_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_delete),
    });
    v.push(crate::mcp::ToolEntry {
        name: "collection_move",
        description: "Move a collection to a new module / parent / position.",
        schema: json!({
            "type": "object",
            "properties": {
                "collection_id": { "type": "string" },
                "new_module_id": { "type": "string" },
                "new_parent_collection_id": { "type": "string" },
                "new_sort_order": { "type": "integer" }
            },
            "required": ["collection_id", "new_module_id"],
            "additionalProperties": false,
        }),
        func: tool_fn!(tool_collection_move),
    });
}

pub(super) async fn tool_collection_list(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: super::ModuleIdArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_list args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.module_id)?,
        "collection_list",
    )
    .await?;
    // 取模块的整棵子树：嵌套集合的父级是另一个集合，「这个模块下的集合」不是一跳
    // 父级就能匹配上的。
    let subtree = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::subtree_ids(c, std::slice::from_ref(&a.module_id))
    })?;
    let in_subtree: std::collections::HashSet<&String> = subtree.iter().collect();
    // 走 `root_id` 索引读该项目的集合再按子树收窄，代价与项目规模成正比，
    // 而不是全表扫。
    let project = owning_project(&state.pool, &a.module_id)?;
    let cols = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::list_by_root_and_type(
            c,
            &project,
            ResourceNodeType::Collection,
        )
    })?;
    Ok(CallToolResult::json(
        cols.into_iter()
            .filter(|n| in_subtree.contains(&n.id))
            .collect::<Vec<_>>(),
    ))
}

pub(super) async fn tool_collection_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_create args: {e}")))?;
    let if_exists = simple_post_core::spec::IfExists::parse(a.if_exists.as_deref().unwrap_or(""))?;

    // `path` 与 `parent_collection_id` 都在说「挂在哪」，两个一起给就是两种说法打架。
    // 悄悄挑一个会让调用方以为建在它说的那个父下 —— 所以直接拒。
    if a.path.as_deref().is_some_and(|p| !p.trim().is_empty()) && a.parent_collection_id.is_some() {
        return Err(AppError::Validation(
            "collection_create: `path` 与 `parent_collection_id` 二选一 —— \
             path 自己就说明了挂在哪"
                .into(),
        ));
    }

    // 集合的父级要么是模块（顶层）要么是集合（嵌套）。`new_module_id` 与
    // `parent_collection_id` 是同一关系的两个名字，取调用方给的那个；`root_id`
    // 由胜出的那个节点推导。
    let parent_id = a
        .parent_collection_id
        .clone()
        .or_else(|| Some(a.module_id.clone()));
    // 按「将来拥有这个新集合的项目」过门，也就是父节点所属的项目。
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, parent_id.as_deref().unwrap_or_default())?,
        "collection_create",
    )
    .await?;

    // 两条写法：给 `path` 就按路径逐级找或建；给 `name` 就建一个。
    let (id, created_collections) = match a.path.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(path) => {
            let segments = simple_post_core::spec::split_path(path)?;
            let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
                simple_post_core::spec::resolve_collection_path(
                    c,
                    &a.module_id,
                    &segments,
                    if_exists,
                )
            })?;
            let last = resolved
                .last()
                .ok_or_else(|| AppError::Validation("集合路径不能为空".into()))?;
            (last.id.clone(), resolved)
        }
        None => {
            let name = a
                .name
                .as_deref()
                .filter(|n| !n.trim().is_empty())
                .ok_or_else(|| {
                    AppError::Validation("collection_create needs a `name` or a `path`".into())
                })?;
            let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
                simple_post_core::spec::resolve_collection(
                    c,
                    &a.module_id,
                    a.parent_collection_id.as_deref(),
                    name,
                    if_exists,
                )
            })?;
            (resolved.id.clone(), vec![resolved])
        }
    };

    let stored =
        simple_post_core::db::with_conn(&state.pool, |c| simple_post_core::db::nodes::get(c, &id))?;
    let mut out = serde_json::to_value(&stored)
        .map_err(|e| AppError::Validation(format!("collection response: {e}")))?;
    out["created"] = json!(created_collections
        .last()
        .map(|r| r.created)
        .unwrap_or(false));
    out["created_collections"] = json!(created_collections
        .into_iter()
        .map(|r| json!({ "id": r.id, "created": r.created }))
        .collect::<Vec<_>>());
    Ok(CallToolResult::json(out))
}

pub(super) async fn tool_collection_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionUpdateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_update args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.collection_id)?,
        "collection_update",
    )
    .await?;
    let patch = ResourceNodePatch {
        parent_id: a
            .patch
            .as_ref()
            .and_then(|p| p.get("parentCollectionId"))
            .map(|v| v.as_str().map(String::from)),
        name: a
            .patch
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(String::from),
        description: a
            .patch
            .as_ref()
            .and_then(|p| p.get("description"))
            .map(|v| v.as_str().map(String::from)),
        sort_order: None,
    };
    let c = simple_post_core::db::with_tx(&state.pool, |c2| {
        simple_post_core::db::nodes::update(c2, &a.collection_id, patch.clone())
    })?;
    Ok(CallToolResult::json(c))
}

pub(super) async fn tool_collection_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let collection_id = arg_str(&args, "collection_id")?.to_string();
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &collection_id)?,
        "collection_delete",
    )
    .await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::delete(c, &collection_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": collection_id })))
}

pub(super) async fn tool_collection_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CollectionMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("collection_move args: {e}")))?;
    crate::mcp::check_permission(
        &state.pool,
        &owning_project(&state.pool, &a.collection_id)?,
        "collection_move",
    )
    .await?;
    // 同样是「两个名字一个关系」：两个都给时嵌套优先 —— 调用方即使只想做嵌套移动，
    // 也仍需带上 module id。
    let new_parent = a
        .new_parent_collection_id
        .clone()
        .or_else(|| Some(a.new_module_id.clone()))
        .unwrap_or_default();
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::move_many(
            c,
            std::slice::from_ref(&a.collection_id),
            Some(&new_parent),
            a.new_sort_order.unwrap_or(0) as i32,
        )
    })?;
    Ok(CallToolResult::json(json!({
        "collection_id": a.collection_id,
        "new_module_id": a.new_module_id,
        "new_parent_collection_id": a.new_parent_collection_id,
    })))
}
