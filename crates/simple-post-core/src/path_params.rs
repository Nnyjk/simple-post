//! URL 路径里的占位符（`/users/{id}`）—— 识别与替换的原语。
//!
//! ## 它与 `{{var}}` 是两套语法
//!
//! `{{var}}` 取自变量池（环境变量与动态变量），`{id}` 取自端点自己的路径参数。
//! 两者住在同一段 URL 文本里，故识别占位符时必须先把 `{{…}}` 整段跳过——
//! 否则 `{{id}}` 会被当成名为 `{id` 的占位符，路径参数与变量池就串了。
//!
//! ## 只在「路径段」里找
//!
//! 占位符只出现在 URL 的路径段：第一个 `?` 或 `#` 之前、主机名之后的那一段。
//! 查询串里的 `{` 是数据（`?filter={status}`），主机名里的 `{` 是别的东西
//! （`http://{tenant}.example.com` 属于主机模板，不归路径参数管）。
//!
//! 对应渲染进程的 `src/lib/path-params.ts`；两侧的规则由
//! `fixtures/path-params-parity.json` 里的路径参数用例钉住。

use std::collections::HashMap;

use crate::url::percent_encode;

/// URL 文本里「路径段」的字节范围：起点在主机名之后，终点在第一个 `?` 或 `#` 之前。
///
/// 没有 `://` 的文本（相对路径）整段都是路径段。
///
/// `://` 只有在**第一个 `?` / `#` 之前**、且**它前面没有 `/`** 时才是 scheme 分隔符。
/// `/users/{id}?next=https://x.com` 里的 `://` 属于查询串：认成 scheme 会把路径段
/// 算成空，于是 `{id}` 既不被替换也不被报出来，请求带着字面量 `{id}` 就发出去了。
fn path_scope(url: &str) -> (usize, usize) {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let start = match url
        .find("://")
        .filter(|at| *at < end && !url[..*at].contains('/'))
    {
        Some(at) => {
            let after_scheme = at + 3;
            if after_scheme >= end {
                // `https://?x` 这种形状：主机名后面没有路径段。
                end
            } else {
                match url[after_scheme..end].find('/') {
                    Some(rel) => after_scheme + rel,
                    // `https://example.com` 后面没有 `/`：整条 URL 就是主机名。
                    None => end,
                }
            }
        }
        None => 0,
    };
    (start, end)
}

/// 占位符名是否合法：非空，且不含空白与 `{ } / ? # &`。
///
/// 斜杠被排除，因为一个路径参数占的是**一段**路径；值里的斜杠由编码处理
/// （`%2F`），写进模板会让路径多出一段。
///
/// 空白按 Unicode 的 White_Space 属性判定（`char::is_whitespace` 判的正是它）。
/// 渲染进程那一侧显式写出同一组码点：JS 的 `\s` 含 U+FEFF 而不含 U+0085，与
/// `is_whitespace` 正好相反，两边用不同的集合就会认出不同的占位符名单。
fn is_placeholder_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| !c.is_whitespace() && !"{}/?#&".contains(c))
}

/// 一个占位符：名字，以及它在路径段里的字节范围（含两头大括号）。
type Found = (String, usize, usize);

/// 扫出路径段里的全部占位符，按出现顺序排列（同名重复出现会各占一条）。
///
/// 扫描器不递归：`{{var}}` 整段跳过，单个 `{name}` 只吃到它自己的那个 `}`。
fn scan(scope: &str) -> Vec<Found> {
    let bytes = scope.as_bytes();
    let mut out: Vec<Found> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i = next_char_boundary(scope, i);
            continue;
        }
        // `{{` 是环境变量的写法：整段跳过，不把它当成路径占位符。
        if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            match find_double_close(scope, i + 2) {
                Some(after) => {
                    i = after;
                    continue;
                }
                None => {
                    i = next_char_boundary(scope, i);
                    continue;
                }
            }
        }
        // 单个大括号：找它配对的那个 `}`。
        match scope[i + 1..].find('}') {
            Some(rel) => {
                let name = &scope[i + 1..i + 1 + rel];
                let end = i + 1 + rel + 1;
                if is_placeholder_name(name) {
                    out.push((name.to_string(), i, end));
                }
                i = end;
            }
            None => {
                i = next_char_boundary(scope, i);
            }
        }
    }
    out
}

/// 从 `from` 起找配对的 `}}`，返回它之后的位置。
fn find_double_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i + 2);
        }
        i = next_char_boundary(s, i);
    }
    None
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// 列出 URL 路径段里的占位符名，按首次出现去重。
///
/// 界面用它列出「这个接口有哪几个路径参数要填」，导入导出侧用它判断
/// 「路径里的占位符有没有人认领」。
pub fn placeholders(url: &str) -> Vec<String> {
    if url.is_empty() {
        return Vec::new();
    }
    let (start, end) = path_scope(url);
    let scope = &url[start..end];
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for (name, _, _) in scan(scope) {
        if seen.insert(name.clone(), ()).is_none() {
            out.push(name);
        }
    }
    out
}

/// 把 URL 路径段里的占位符替换成给定的值。
///
/// `values` 是「占位符名 → 取到的值」，值是**未经编码**的原文：编码由本函数
/// 统一做，字符集取 [`percent_encode`]（与渲染进程的 `encodeURIComponent`
/// 同一套），故路径参数的值与查询参数的值遵守同一条编码纪律。
///
/// 取不到值的占位符**原样留在 URL 里**，并出现在返回的 `missing` 里。
/// 值是空串或只有空白时同样按「取不到」处理：路径参数占的是一段路径，替换成空串
/// 只会把路径截断，而一个只有空白的值是用户没填完，不是用户想发一个空格。
///
/// 返回值第一项是替换后的 URL，第二项是取不到值的占位符名（按首次出现去重）。
pub fn substitute(url: &str, values: &[(String, String)]) -> (String, Vec<String>) {
    if url.is_empty() {
        return (String::new(), Vec::new());
    }
    let (start, end) = path_scope(url);
    let scope = &url[start..end];
    let found = scan(scope);
    if found.is_empty() {
        return (url.to_string(), Vec::new());
    }

    let mut out = String::with_capacity(url.len());
    out.push_str(&url[..start]);
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut cursor = 0usize;
    for (name, from, to) in &found {
        out.push_str(&scope[cursor..*from]);
        match values.iter().find(|(k, _)| k == name) {
            Some((_, value)) if !value.trim().is_empty() => {
                out.push_str(&percent_encode(value));
            }
            _ => {
                if seen.insert(name.clone(), ()).is_none() {
                    missing.push(name.clone());
                }
                // 取不到值就把占位符原样放回去，用户在 URL 预览里看得见缺哪个。
                out.push_str(&scope[*from..*to]);
            }
        }
        cursor = *to;
    }
    out.push_str(&scope[cursor..]);
    out.push_str(&url[end..]);
    (out, missing)
}

// 测试——占位符的识别范围与替换规则。

#[cfg(test)]
#[path = "path_params_test.rs"]
mod tests;
