//! `{{var}}` 替换原语 —— 底层积木，其余模块都在它之上做拼装。
//!
//! 对应渲染进程 `src/lib/url.ts` 的 `resolveVars`，正则认 `\w+`，两边必须一致。
//!
//! 住在 core 而非调用方：导入器要把外来格式里的 `{{var}}` 形状与这里比对（Postman 会写
//! `{{my-var}}` 与 `{{$timestamp}}`，两者都**不是** word，必须成为诊断而不是静默放过），
//! 而依赖方向是 `interchange → core`，反向的写法不成立。

use std::collections::HashMap;

/// `{{var}}` 替换的结果。`missing` 已去重并按出现顺序排列，调用方可以直接拿它渲染
/// 「未解析变量」列表。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTemplate {
    pub resolved: String,
    pub missing: Vec<String>,
}

// -------------------------------------------------------------------------
// resolve_template —— `{{var}}` 替换原语
// -------------------------------------------------------------------------

/// 用 `vars` 替换 `template` 里的每个 `{{name}}` 标记。
///
/// 未知名**原样保留**成 `{{name}}`（不变成 "undefined"、也不变空），这样请求预览能
/// 显示到底缺了什么。
///
/// 标记名不支持非字母数字（`{{ my-var }}` 也不行）：正则只认 `\w+` =
/// `[A-Za-z0-9_]`，与渲染进程接受的是同一套。以 TS 侧的正则
/// `/\\{\\{(\\w+)\\}\\}/g` 为准。
pub fn resolve_template(template: &str, vars: &HashMap<String, String>) -> ResolvedTemplate {
    if template.is_empty() {
        return ResolvedTemplate {
            resolved: String::new(),
            missing: Vec::new(),
        };
    }

    let mut missing: Vec<String> = Vec::new();
    let mut seen_missing: HashMap<String, ()> = HashMap::new();

    let resolved = template_replace(template, |key| match vars.get(key) {
        Some(v) => v.clone(),
        None => {
            if seen_missing.insert(key.to_string(), ()).is_none() {
                missing.push(key.to_string());
            }
            format!("{{{{{key}}}}}")
        }
    });

    ResolvedTemplate { resolved, missing }
}

/// 遍历 `template` 里的 `{{...}}` 标记，由 `repl` 决定各自变成什么。内层扫描器刻意
/// 不递归（`{{{{a}}}}` 第一趟得到 `{{a}}`，之后若 `a` 可解析再展开），所以叠写的标记
/// 不可能爆栈。
fn template_replace<F: FnMut(&str) -> String>(template: &str, mut repl: F) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // 找配对的 `}}`。
            if let Some(end_rel) = find_close(template, i + 2) {
                let inner = &template[i + 2..i + 2 + end_rel];
                // 标记内容必须是裸标识符（`\w+`），与渲染进程的正则一致。其它形状
                // （`{{ foo }}` 带空格、`{{ foo.bar }}`）原样放过 —— TS 侧同样如此。
                if is_word(inner) {
                    out.push_str(&repl(inner));
                } else {
                    out.push_str(&template[i..i + 2 + end_rel + 2]);
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }
        // 按 UTF-8 字符（不是字节）推进，避免切在 rune 中间。
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    out
}

fn find_close(s: &str, from: usize) -> Option<usize> {
    // 从 `from` 开始在字符边界上找 `}}`。
    let mut i = from;
    while i + 1 < s.len() {
        if s.as_bytes()[i] == b'}' && s.as_bytes()[i + 1] == b'}' {
            return Some(i - from);
        }
        i = next_char_boundary(s, i);
    }
    None
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// 抽出 `s` 里所有 `{{name}}` 标记名。`resolve_endpoint` 用它汇总 URL / headers /
/// params / body / auth 各处提到的变量，调用方不必为「缺了哪些变量」再扫一遍。
pub fn list_missing_vars(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end_rel) = find_close(s, i + 2) {
                let inner = &s[i + 2..i + 2 + end_rel];
                if is_word(inner) && seen.insert(inner.to_string(), ()).is_none() {
                    out.push(inner.to_string());
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }
        i = next_char_boundary(s, i);
    }
    out
}

// 测试——随实现一起搬到 core。

#[cfg(test)]
#[path = "template_test.rs"]
mod tests;
