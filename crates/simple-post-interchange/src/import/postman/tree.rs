//! 树遍历：显式工作栈走 folder 树，以及「一个条目是请求还是文件夹」的分类。

use serde_json::{Map, Value};

use crate::diagnostics::Diagnostic;
use crate::ir::{IrCollection, IrEndpoint, IrServer};

use super::request::build_endpoint;
use super::{description_text, item_name, Ctx};

/// 一个条目在源文件里的形状。
pub(super) enum Class<'a> {
    Request(&'a Map<String, Value>),
    Folder(&'a [Value]),
    Malformed,
}

/// 一层容器的结果。
#[derive(Default)]
pub(super) struct Container {
    pub(super) endpoints: Vec<IrEndpoint>,
    pub(super) collections: Vec<IrCollection>,
}

/// 显式工作栈的一层。
///
/// 递归遍历一棵深度无上限的 folder 树，会让畸形外来文件靠栈深度把进程带走；
/// 落库侧（`ir::endpoint_keys_of`）也是显式栈，两侧同一条纪律。
struct Frame<'a> {
    /// 本容器在源文件里的指针。子项是 `{key}/item/{i}`。
    key: String,
    name: String,
    description: Option<String>,
    /// 拥有本层的那一层的栈下标，`None` 为根层。
    ///
    /// 必须记下标而不是收尾时取 `last_mut()`：一个 folder 没走完时栈顶是**它自己**，
    /// 它的兄弟条目会被挂进这个 folder 里去。
    parent: Option<usize>,
    items: &'a [Value],
    idx: usize,
    endpoints: Vec<IrEndpoint>,
    collections: Vec<IrCollection>,
}

/// 遍历一层容器的全部子项，返回这一层的 endpoints 与 collections。
pub(super) fn walk_children<'a>(
    items: &'a [Value],
    ptr: &str,
    servers: &mut Vec<IrServer>,
    ctx: &mut Ctx,
) -> Container {
    let mut stack: Vec<Frame<'a>> = vec![Frame {
        key: ptr.to_string(),
        name: String::new(),
        description: None,
        parent: None,
        items,
        idx: 0,
        endpoints: Vec::new(),
        collections: Vec::new(),
    }];

    loop {
        // 一次借用同时拿到下标与本层：`stack.len()` 与 `last_mut()` 分两次做会撞借检查。
        let Some((owner, top)) = stack.len().checked_sub(1).zip(stack.last_mut()) else {
            return Container::default();
        };
        if top.idx >= top.items.len() {
            let Some(done) = stack.pop() else {
                return Container::default();
            };
            let child = IrCollection {
                key: done.key,
                name: done.name,
                description: done.description,
                endpoints: done.endpoints,
                collections: done.collections,
            };
            match done.parent {
                Some(owner) => stack[owner].collections.push(child),
                None => {
                    return Container {
                        endpoints: child.endpoints,
                        collections: child.collections,
                    }
                }
            }
            continue;
        }

        let idx = top.idx;
        top.idx += 1;
        let items_ref: &'a [Value] = top.items;
        let parent_key = top.key.clone();
        let item = &items_ref[idx];
        let item_ptr = format!("{parent_key}/item/{idx}");

        let Some(obj) = item.as_object() else {
            ctx.diagnostics
                .push(Diagnostic::error(&item_ptr, "这一项不是 JSON 对象，已跳过"));
            continue;
        };

        match classify(obj, &item_ptr, ctx) {
            Class::Request(req) => {
                let name = item_name(obj);
                if let Some(ep) = build_endpoint(req, name, &item_ptr, servers, ctx) {
                    stack[owner].endpoints.push(ep);
                }
            }
            Class::Folder(children) => stack.push(Frame {
                key: item_ptr,
                name: item_name(obj),
                description: description_text(obj.get("description")),
                parent: Some(owner),
                items: children,
                idx: 0,
                endpoints: Vec::new(),
                collections: Vec::new(),
            }),
            Class::Malformed => {}
        }
    }
}

/// 一个条目是请求还是文件夹。
///
/// `request` 优先于 `item`：两者同时存在时（Postman 的 schema 允许「带请求的文件夹」）
/// 取请求，子项会丢，故出一条诊断。
pub(super) fn classify<'a>(obj: &'a Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> Class<'a> {
    if let Some(req) = obj.get("request") {
        if let Some(arr) = obj
            .get("item")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        {
            ctx.diagnostics.push(
                Diagnostic::warn(ptr, "该请求条目下还挂着子项，子项不会被导入")
                    .dropping(format!("{} 个子项", arr.len())),
            );
        }
        return match req.as_object() {
            Some(m) => Class::Request(m),
            None => {
                ctx.diagnostics.push(Diagnostic::error(
                    format!("{ptr}/request"),
                    "request 不是 JSON 对象，已跳过这一条",
                ));
                Class::Malformed
            }
        };
    }

    match obj.get("item") {
        Some(Value::Array(arr)) => Class::Folder(arr),
        _ => {
            ctx.diagnostics.push(Diagnostic::error(
                ptr,
                "这一项既没有 request 也没有 item 数组，已跳过",
            ));
            Class::Malformed
        }
    }
}
