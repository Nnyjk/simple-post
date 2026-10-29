//! `$ref` 展开：显式工作栈遍历，命中访问栈即成环，超深度即截断。

use serde_json::{Map, Value};

use crate::diagnostics::Diagnostic;
use crate::spec::escape_pointer_token;

use super::{Ctx, MAX_REF_DEPTH};

/// 一个待展开的槽：对象的键，或「数组里的下一个」。数组下标不记，因为展开失败的那
/// 几条会被丢掉，剩下的按原顺序压紧即可。
enum Slot {
    Key(String),
    Index,
}

/// 一条待展开的任务。`input` 自带一份副本：工作栈上跨迭代存活的值不能是借用，
/// 否则子任务就得指着一个每轮都被换掉的局部变量。
struct Task {
    input: Value,
    pointer: String,
    depth: usize,
    id: usize,
}

/// 工作栈上的一帧。
enum Frame {
    Task(Task),
    /// 子任务都回来了，按槽位拼出这个节点自己的值。
    Merge {
        id: usize,
        slots: Vec<(Slot, usize)>,
        base: Value,
        /// 本帧压进 [`Ctx::active`] 的 ref 指针，合并完弹回。
        opened: Vec<String>,
    },
}

/// 把一棵子树里的 `$ref` 全部展开掉。
///
/// 命中访问栈即成环，超深度即截断，两种情况都只截断**那一支**并出 `Warn`。
/// 显式工作栈而非递归：外来文档的嵌套深度不受本仓库约束。
pub(super) fn deref(ctx: &mut Ctx, root: &Value, input: &Value, pointer: &str) -> Option<Value> {
    let mut results: Vec<Option<Value>> = Vec::new();
    let root_id = results.len();
    results.push(None);
    let mut stack = vec![Frame::Task(Task {
        input: input.clone(),
        pointer: pointer.to_string(),
        depth: 0,
        id: root_id,
    })];

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Task(task) => {
                // 展开失败时 `resolve` 没有把任何指针压进访问栈，故不必回退。
                let Some((node, opened)) =
                    resolve(ctx, root, &task.input, &task.pointer, task.depth)
                else {
                    continue;
                };
                // 本帧压进去的 ref 指针：非空对象/数组由下面的 Merge 帧弹回，标量就地弹回。
                // 两侧必须对称——少弹一次会提前清掉祖先帧的「正在展开」，那棵子树里
                // 真的成环时就认不出来了，只能靠深度上限拦住，报错会指向错误的方向。
                ctx.active.extend(opened.iter().cloned());
                match node {
                    Value::Object(map) if !map.is_empty() => {
                        let children: Vec<(String, Value)> = map.into_iter().collect();
                        let mut slots = Vec::with_capacity(children.len());
                        for (key, _) in &children {
                            slots.push((Slot::Key(key.clone()), results.len()));
                            results.push(None);
                        }
                        // Merge 先入栈：栈是后进先出，故它落在**所有**子任务之下，
                        // 也就是它们都回来之后才轮到它。
                        let ids: Vec<usize> = slots.iter().map(|(_, id)| *id).collect();
                        stack.push(Frame::Merge {
                            id: task.id,
                            slots,
                            base: Value::Object(Map::new()),
                            opened,
                        });
                        for (index, (key, child)) in children.into_iter().enumerate() {
                            stack.push(Frame::Task(Task {
                                input: child,
                                pointer: format!("{}/{}", task.pointer, escape_pointer_token(&key)),
                                depth: task.depth + 1,
                                id: ids[index],
                            }));
                        }
                    }
                    Value::Array(items) if !items.is_empty() => {
                        let mut slots = Vec::with_capacity(items.len());
                        for _ in 0..items.len() {
                            slots.push((Slot::Index, results.len()));
                            results.push(None);
                        }
                        let ids: Vec<usize> = slots.iter().map(|(_, id)| *id).collect();
                        stack.push(Frame::Merge {
                            id: task.id,
                            slots,
                            base: Value::Array(Vec::new()),
                            opened,
                        });
                        for (index, child) in items.into_iter().enumerate() {
                            stack.push(Frame::Task(Task {
                                input: child,
                                pointer: format!("{}/{index}", task.pointer),
                                depth: task.depth + 1,
                                id: ids[index],
                            }));
                        }
                    }
                    scalar => {
                        release(ctx, &opened);
                        results[task.id] = Some(scalar);
                    }
                }
            }
            Frame::Merge {
                id,
                slots,
                mut base,
                opened,
            } => {
                // 展开失败（成环 / 指不到）的槽位不填，等于丢掉那一条；数组因此变短。
                for (slot, child) in slots {
                    let Some(value) = results[child].take() else {
                        continue;
                    };
                    match (&mut base, slot) {
                        (Value::Object(map), Slot::Key(key)) => {
                            map.insert(key, value);
                        }
                        (Value::Array(items), Slot::Index) => items.push(value),
                        _ => {}
                    }
                }
                release(ctx, &opened);
                results[id] = Some(base);
            }
        }
    }
    results[root_id].take()
}

/// 沿 `$ref` 走查出真实节点，并把 `$ref` 旁边的兄弟字段合并上去（兄弟优先，3.1 语义）。
///
/// 返回值第二项是本次压进访问栈的指针，由调用方在子树展开完后弹回。
fn resolve(
    ctx: &mut Ctx,
    root: &Value,
    node: &Value,
    pointer: &str,
    depth: usize,
) -> Option<(Value, Vec<String>)> {
    let mut opened: Vec<String> = Vec::new();
    let mut current = node.clone();
    let mut hops = 0usize;

    loop {
        let Value::Object(map) = &current else {
            return Some((current, opened));
        };
        let Some(Value::String(reference)) = map.get("$ref") else {
            return Some((current, opened));
        };
        hops += 1;
        if depth + hops > MAX_REF_DEPTH {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 嵌套太深，已截断该分支")
                    .dropping(reference.clone()),
            );
            return None;
        }
        if opened.iter().any(|p| p == reference) || ctx.active.contains(reference) {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 成环，已停止展开该分支")
                    .dropping(reference.clone()),
            );
            return None;
        }
        if !reference.starts_with('#') {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "外部文件里的 $ref 无法解析").dropping(reference.clone()),
            );
            return None;
        }
        let Some(target) = pointer_lookup(root, reference) else {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 指不到东西，该分支已丢弃")
                    .dropping(reference.clone()),
            );
            return None;
        };

        opened.push(reference.clone());
        // 兄弟字段覆盖被指向的内容（3.1 语义）。3.0 的工具链也这么写，故两条都收。
        let mut merged = target.clone();
        if let Some(target_map) = merged.as_object_mut() {
            for (key, value) in map {
                if key != "$ref" {
                    target_map.insert(key.clone(), value.clone());
                }
            }
        }
        current = merged;
    }
}

/// `#/components/schemas/Foo` 逐段走查。`~1` 是 `/`，`~0` 是 `~`。
fn pointer_lookup<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let rest = reference.strip_prefix("#/")?;
    let mut current = root;
    for raw in rest.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        current = current.get(&token)?;
    }
    Some(current)
}

/// 把本帧压进去的 ref 指针弹回访问栈。
fn release(ctx: &mut Ctx, opened: &[String]) {
    for _ in opened {
        ctx.active.pop();
    }
}
