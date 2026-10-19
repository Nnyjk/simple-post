//! 导入 / 导出 Tauri 薄壳的**线契约**：一条键名，三处必须一致。
//!
//! | 边 | 形状由谁决定 |
//! |---|---|
//! | 渲染层 `useTransfer` | 手写的扁平对象字面量 |
//! | Tauri 命令 | 参数名，宏按 `to_lower_camel_case` 取 payload 键（`ArgumentCase::Camel` 是默认） |
//! | REST 薄壳 | `Json<*Opts>` 整个 body，键名是 `#[serde(rename_all = "camelCase")]` |
//!
//! 为什么要读源码而不是断言行为：这条不变量坏掉时的形态很难看。
//!
//! * 浏览器形态 100% 正常 —— `POST /api/interchange/import/preview` 收到的就是那个 body。
//! * 桌面形态每个导入都报 `invalid args … missing required key opts`，而所有 Rust 测试、
//!   前端测试与 `npm run check` 全绿。
//!
//! 缺陷的整个可观测面在 IPC 那一侧，进程内没有任何东西能碰到它。这与
//! `daemon_supervisor_test.rs::the_shared_spawn_path_stays_free_of_tauri` 同理：那里断言的
//! 性质也没有行为特征（它的行为就是测试二进制起不来）。三处都比对自己那份源码，故期望值
//! 永远只有一个来源（`dto.rs`），不会像手写白名单那样随字段增删而腐化。
//!
//! REST 那半边不在这儿测——它有真实的行为级覆盖：
//! `crates/simple-post-server/src/web_api/routes/interchange_test.rs` 拿渲染层发的那个
//! 扁平 body 打四个端点。

use serde_json::json;

use crate::dto::{ExportDocumentOpts, ExportPreviewOpts, ImportCommitOpts, ImportPreviewOpts};

/// 命令签名。
const CMDS: &str = include_str!("interchange.rs");
/// `*Opts` 的定义处。两种 transport 的线格式共同的事实来源。
const DTO: &str = include_str!("../../../crates/simple-post-server/src/dto.rs");
/// 渲染层的调用点。
const HOOK: &str = include_str!("../../../src/features/interchange/useTransfer.ts");

/// 与 `tauri-macros` 的 `ToLowerCamelCase` 对齐：命令参数名 → payload 键。
fn camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// 按顶层逗号切分。`<` / `(` 会把深度抬上去，故 `Option<Vec<String>>` 不会把参数切成两半。
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, ch) in s.char_indices() {
        match ch {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// 一个命令的参数名（`state` 除外 —— 它由 Tauri 自己注入，不是线上的键），已转 camelCase。
fn command_params(src: &str, name: &str) -> Vec<String> {
    let sig = format!("fn {name}(");
    let start = src
        .find(&sig)
        .unwrap_or_else(|| panic!("`{name}` must stay a whole function in interchange.rs"))
        + sig.len();
    // 签名以 `(` 开头的那一个已被 `sig` 吃掉，故深度从 0 计，第一个落地的 `)` 就是收口。
    let rest = &src[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("`{name}` parameter list must close"));
    let mut keys: Vec<String> = split_top_level(&rest[..end])
        .iter()
        .filter_map(|p| p.split_once(':').map(|(n, _)| camel(n.trim())))
        .filter(|n| n != "state")
        .collect();
    keys.sort();
    keys
}

/// 一个 `*Opts` 的字段名（Rust 侧原样，snake_case）。
fn opts_fields(src: &str, name: &str) -> Vec<String> {
    let sig = format!("pub struct {name} {{");
    let start = src
        .find(&sig)
        .unwrap_or_else(|| panic!("`{name}` must exist in dto.rs"))
        + sig.len();
    let body = &src[start..];
    let end = body
        .find("\n}")
        .unwrap_or_else(|| panic!("`{name}` body must close"));
    body[..end]
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("pub ")
                .and_then(|r| r.split_once(':').map(|(n, _)| n.trim().to_string()))
        })
        .map(|f| camel(&f))
        .collect()
}

/// `useTransfer` 里某次 `invokeCmd` 第二个参数的对象字面量，扫出全部键。
///
/// 两种写法都算键：`key: value`，以及 TS 的简写 `key,`。后者只在**顶层**认 —— 值里
/// 的标识符（`selected: [...selected]` 里那个 `selected`）都在括号里。
///
/// 嵌套不限深度是刻意的：可选参数写成 `...(args.content ? { content: args.content } : {})`，
/// 那个 `content` 就在嵌套一层里，只看顶层会把这两项读成缺席。字面量内部没有别的
/// `ident:` / `ident,` 形状——三元条件不带冒号，`s.projectId` 后面也不带——故扫出来的
/// 就是键。
fn hook_args(src: &str, command: &str) -> Vec<String> {
    let anchor = format!("COMMANDS.{command}, {{");
    let start = src
        .find(&anchor)
        .unwrap_or_else(|| panic!("`{command}` must be invoked from useTransfer.ts"))
        + anchor.len()
        - 1;
    let rest = &src[start..];
    // 必须配平：字面量里的 `...(args.content ? { content: args.content } : {})` 先遇到的是
    // 内层那个 `}`，按「第一个 `}`」截会把 `url` 那半截切掉。
    let mut depth = 0i32;
    let end = rest
        .find(|ch| match ch {
            '{' | '[' | '(' => {
                depth += 1;
                false
            }
            '}' | ']' | ')' => {
                depth -= 1;
                depth == 0 && matches!(ch, '}')
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("`{command}` argument object must close"));
    // 去掉字面量自己的那对花括号，键才落在深度 0 上。
    let literal = &rest[1..end];
    let bytes = literal.as_bytes();
    let mut keys: Vec<String> = Vec::new();
    // 顶层位置 + 是否已经吃过冒号。`key: value` 的 **value** 也在深度 0 上（`content:
    // sourceText`），所以光看深度会把值读成简写键。
    let (mut i, mut depth, mut in_value) = (0usize, 0i32, false);
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '{' | '[' | '(' => {
                depth += 1;
                i += 1;
            }
            '}' | ']' | ')' => {
                depth -= 1;
                i += 1;
            }
            ',' if depth == 0 => {
                in_value = false;
                i += 1;
            }
            ':' if depth == 0 => {
                in_value = true;
                i += 1;
            }
            _ if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' => {
                let name_start = i;
                while i < bytes.len()
                    && ((bytes[i] as char).is_ascii_alphanumeric()
                        || bytes[i] == b'_'
                        || bytes[i] == b'$')
                {
                    i += 1;
                }
                let mut j = i;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                let next = bytes.get(j).copied().map(|b| b as char);
                let is_key = match next {
                    // `key:` 在任何深度都算：嵌套那层里的可选参数就靠这条认出来。
                    Some(':') => true,
                    // 简写 `key,` / `key}` 只认顶层且不在值位置上。
                    Some(',') | Some('}') if depth == 0 && !in_value => true,
                    _ => false,
                };
                if is_key {
                    keys.push(literal[name_start..i].to_string());
                }
            }
            _ => i += 1,
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

/// 期望的线上键名。`dto.rs` 是唯一来源，另两条边都要与它相等。
fn wire_keys(opts: &str) -> Vec<String> {
    let mut keys = opts_fields(DTO, opts);
    keys.sort();
    keys
}

#[test]
fn every_command_takes_the_opts_fields_as_flat_arguments() {
    for (cmd, opts) in [
        ("interchange_import_preview", "ImportPreviewOpts"),
        ("interchange_import_commit", "ImportCommitOpts"),
        ("interchange_export_preview", "ExportPreviewOpts"),
        ("interchange_export_document", "ExportDocumentOpts"),
    ] {
        assert_eq!(
            command_params(CMDS, cmd),
            wire_keys(opts),
            "`{cmd}` must spell its arguments out one by one, named after the \
             `{opts}` fields. A single `opts: {opts}` parameter compiles, leaves \
             every Rust test green and the web shape working, and then fails every \
             desktop import with `missing required key opts` — the macro binds by \
             parameter name, it never deserializes the payload."
        );
    }
}

#[test]
fn the_renderer_sends_exactly_those_keys() {
    for (cmd, opts) in [
        ("interchangeImportPreview", "ImportPreviewOpts"),
        ("interchangeImportCommit", "ImportCommitOpts"),
        ("interchangeExportPreview", "ExportPreviewOpts"),
        ("interchangeExportDocument", "ExportDocumentOpts"),
    ] {
        assert_eq!(
            hook_args(HOOK, cmd),
            wire_keys(opts),
            "`{cmd}` is invoked with a different key set than `{opts}` declares. \
             Fix the call site rather than the command: wrapping the payload in an \
             `opts` key repairs the desktop shape and silently breaks the web one."
        );
    }
}

/// 这些键名必须是**真正的**线上名，而不只是源码里长得像。去掉
/// `#[serde(rename_all = "camelCase")]` 时上面三条仍会绿（它们比的都是自己算出来的名字），
/// 所以这里让 serde 自己说：REST body 发过去必须真的落到字段上。
#[test]
fn the_wire_keys_are_the_camel_case_field_names() {
    let preview: ImportPreviewOpts = serde_json::from_value(json!({
        "projectId": "p1",
        "moduleId": "m1",
        "content": "{}",
    }))
    .expect("a flat camelCase body deserializes");
    assert_eq!(preview.project_id, "p1");
    assert_eq!(preview.module_id, "m1");
    assert_eq!(preview.content.as_deref(), Some("{}"));
    assert!(
        preview.url.is_none(),
        "an absent key is `None`, not an error"
    );

    let commit: ImportCommitOpts = serde_json::from_value(json!({
        "projectId": "p1",
        "moduleId": "m1",
        "content": "{}",
        "selected": ["a", "b"],
        "applyService": false,
    }))
    .expect("commit body deserializes");
    assert_eq!(commit.project_id, "p1");
    assert_eq!(commit.selected, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(
        commit.apply_service,
        Some(false),
        "`false` must reach the field; a body that drops it would silently read as \
         the default, and the desktop path would build a service the browser path does not"
    );

    let export_preview: ExportPreviewOpts =
        serde_json::from_value(json!({ "projectId": "p1", "moduleId": "m1" }))
            .expect("export preview body deserializes");
    assert_eq!(export_preview.module_id, "m1");

    let document: ExportDocumentOpts = serde_json::from_value(json!({
        "projectId": "p1",
        "moduleId": "m1",
        "environmentId": "",
        "selected": [],
        "format": "yaml",
    }))
    .expect("export document body deserializes");
    assert_eq!(document.environment_id, "");
    assert_eq!(document.format, "yaml");
}
