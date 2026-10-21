//! `event[]`：Postman 的两段脚本（前置 / 测试）。
//!
//! Postman 把脚本挂在 `event[]` 上，每一项有 `listen`（相位）与 `script.exec`
//! （源码）：`prerequest` 是请求发出前跑的那段，`test` 是响应回来后跑的那段。
//! 同一个相位可以有**多项**，它们依次执行。
//!
//! **只认这两个相位。** 别的 `listen`（典型是测试运行器的监听器）在本仓库没有
//! 对应构造，出诊断说明，不静默丢。
//!
//! 脚本源码**不写进诊断**：用户的脚本里常有 token 与环境变量名，诊断是要显示给人
//! 看的，不该把源码带出去。

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::diagnostics::Diagnostic;
use crate::ir::IrScripts;

use super::{field_text, Ctx};

/// 脚本挂在哪里。
///
/// 「有归宿」与「没有归宿」是两种**不同**的结果，不是一条诊断的两种措辞：
/// 有归宿的那些被**导入**，没有的那些逐条出诊断说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// 挂在接口或集合上。本仓库的脚本就住在这两处，故有归宿。
    Node,
    /// 整份集合的根。**没有归宿**：本仓库的脚本住在接口与集合上，而「整份集合」
    /// 不是一个节点——顶层条目各自成一个模块，挂进已有模块时那些模块降级成集合，
    /// 根上的脚本挂不到任何一个上。
    CollectionRoot,
}

/// 一个条目（或集合根）上的 `event[]` → 两段脚本。
///
/// `ptr` 是这个条目在源文件里的位置指针，诊断的指针在它后面接 `/event/{i}`。
pub(super) fn parse_scripts(
    obj: &Map<String, Value>,
    ptr: &str,
    place: Place,
    ctx: &mut Ctx,
) -> IrScripts {
    let mut scripts = IrScripts::default();
    let Some(events) = obj.get("event").and_then(Value::as_array) else {
        return scripts;
    };

    let mut pre: Vec<String> = Vec::new();
    let mut test: Vec<String> = Vec::new();
    // 已经报过的脚本源码。同一段脚本出现十次是一个问题，不是十个——与
    // `Ctx::seen_bad_tokens` 那套去重纪律一样。
    let mut reported: HashSet<String> = HashSet::new();
    for (i, event) in events.iter().enumerate() {
        let at = format!("{ptr}/event/{i}");
        let Some(event) = event.as_object() else {
            ctx.diagnostics.push(
                Diagnostic::warn(&at, "这一条 event 不是 JSON 对象，已跳过").dropping("event"),
            );
            continue;
        };

        // `disabled: true` 是作者关掉了这段脚本：它在 Postman 里不跑，导入后也不能
        // 变成会跑的（它能改写请求、能写环境变量）。
        if event.get("disabled").and_then(Value::as_bool) == Some(true) {
            ctx.diagnostics.push(
                Diagnostic::info(&at, "这一条 event 在 Postman 里是禁用的，未导入")
                    .dropping("禁用的脚本"),
            );
            continue;
        }

        let Some(source) = exec_source(event, &at, ctx) else {
            continue;
        };
        // 空脚本就是没有脚本，不是错误。
        if source.trim().is_empty() {
            continue;
        }

        if place == Place::CollectionRoot {
            if reported.insert(source) {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        &at,
                        "整份集合上的脚本没有落点：顶层条目各自成一个分组，而本仓库的脚本只挂在接口与集合上",
                    )
                    .dropping("集合级脚本"),
                );
            }
            continue;
        }

        match field_text(event, "listen").unwrap_or_default().as_str() {
            "prerequest" => pre.push(source),
            "test" => test.push(source),
            "" => ctx.diagnostics.push(
                Diagnostic::warn(&at, "这一条 event 没有 listen（事件类型），已跳过")
                    .dropping("event"),
            ),
            other => ctx.diagnostics.push(
                Diagnostic::warn(
                    &at,
                    format!("未知的事件类型 {other:?}，本仓库只有前置脚本与测试脚本两个相位"),
                )
                .dropping(format!("event {other}")),
            ),
        }
    }

    scripts.pre_request_script = join(pre);
    scripts.test_script = join(test);
    scripts
}

/// `event.script.exec` → 源码。
///
/// Postman 把它写成**字符串数组**（一行一段），也有写成单个字符串的。两者读出来
/// 都是一段源码；数组里混进非字符串时整条读不出来（不静默丢一半），出诊断。
fn exec_source(event: &Map<String, Value>, at: &str, ctx: &mut Ctx) -> Option<String> {
    let Some(script) = event.get("script").and_then(Value::as_object) else {
        ctx.diagnostics
            .push(Diagnostic::warn(at, "这一条 event 没有 script，已跳过").dropping("event"));
        return None;
    };
    let Some(exec) = script.get("exec") else {
        ctx.diagnostics.push(
            Diagnostic::warn(at, "这一条 event 的 script 没有 exec，已跳过").dropping("event"),
        );
        return None;
    };

    match exec {
        Value::String(s) => Some(s.clone()),
        Value::Array(lines) => {
            let mut out = Vec::with_capacity(lines.len());
            for line in lines {
                let Some(s) = line.as_str() else {
                    ctx.diagnostics.push(
                        Diagnostic::warn(
                            at,
                            "这一条 event 的脚本源码里有不是字符串的项，整条已跳过",
                        )
                        .dropping("event"),
                    );
                    return None;
                };
                out.push(s);
            }
            Some(out.join("\n"))
        }
        _ => {
            ctx.diagnostics.push(
                Diagnostic::warn(at, "这一条 event 的脚本源码不是文本，已跳过").dropping("event"),
            );
            None
        }
    }
}

/// 同一相位的多项脚本拼成一段。
///
/// 每一项各自**分块**包起来：Postman 里它们是各自独立的脚本，同名 `const` / `let`
/// 在同一层作用域里撞上会让整段抛 `SyntaxError`（两段都不跑），而一段不以分号结尾
/// 加上下一段以 `(` 或 `[` 开头会被自动分号插入并成一句。
fn join(sources: Vec<String>) -> Option<String> {
    if sources.is_empty() {
        return None;
    }
    if sources.len() == 1 {
        return sources.into_iter().next();
    }
    Some(
        sources
            .iter()
            .map(|s| format!("{{\n{s}\n}}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
