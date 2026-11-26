//! 四处注册表的差集，逐条钉成断言。
//!
//! dual-transport 的纪律是「同一条命令要在五处各留一行」。五处里没有任何两处共享源码，
//! 于是唯一能自动断言的就是**文本里的条目集合**：`include_str!` 读四份源码，算精确差集，
//! 白名单里的每一条都要在这里写明它为什么不算缺口。改了任一侧而漏改另一侧，这里先红。
//!
//! 五处的第五处（`src-tauri/src/commands/*` 的处理函数）由编译器守：`generate_handler!`
//! 里写了不存在的路径就编不过；`project_api_shells_test.rs` 再钉住薄壳的形状。
//!
//! 解析都是手写的（不引 `regex`）：两侧的写法就那几种，写得笨一点反而不会被格式变化骗过。
//! 注意「命令名」与「路由键」是两套名字：`commands.ts` 的键是 camelCase（`projectsList`），
//! 值才是 Rust 侧的命令名（`projects_list`）；`web-routes.ts` 的键又是 camelCase。

use std::collections::{BTreeMap, BTreeSet};

/// 一条路由：方法 + 归一后的路径模板。
pub(crate) type Route = (String, String);

const COMMANDS_TS: &str = include_str!("../../../src/lib/tauri-commands/commands.ts");
const WEB_ROUTES_TS: &str = include_str!("../../../src/lib/tauri-commands/web-routes.ts");
const DESKTOP_LIB_RS: &str = include_str!("../../../src-tauri/src/lib.rs");
const ROUTER_RS: &str = include_str!("web_api/router.rs");

// ----- 解析 ---------------------------------------------------------------

fn chars_of(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// 在字符序列里找 `pat` 的第一次出现（起点 ≥ `from`）。
fn find_seq(chars: &[char], pat: &str, from: usize) -> Option<usize> {
    let pat_chars = chars_of(pat);
    if pat_chars.is_empty() || pat_chars.len() > chars.len() {
        return None;
    }
    (from..=chars.len() - pat_chars.len()).find(|&i| chars[i..i + pat_chars.len()] == pat_chars[..])
}

/// `text` 里 `start` 与 `end` 之间的那一段（两端都不含）。
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("源码里找不到 `{start}` —— 它改名了？"));
    let rest = &text[from + start.len()..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("源码里找不到 `{end}` —— 它改名了？"));
    &rest[..to]
}

/// 从 `chars[from..]` 起的第一段引号字面量：返回 (内容, 收尾引号之后的下标)。
///
/// 模板字符串里的 `${…}` **原样保留**（含 `${`），交给 `normalize_path` 决定怎么归一。
fn string_at(chars: &[char], from: usize) -> Option<(String, usize)> {
    let mut i = from;
    while i < chars.len() && !matches!(chars[i], '\'' | '"' | '`') {
        i += 1;
    }
    if i == chars.len() {
        return None;
    }
    let quote = chars[i];
    i += 1;
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == quote {
            return Some((out, i + 1));
        }
        if quote == '`' && c == '$' && chars.get(i + 1) == Some(&'{') {
            // 原样复制 `${…}`：`$` 自己也要进 out，否则归一化时会剩下光秃秃的 `$`。
            out.push('$');
            i += 1; // 移到 `{`，让下面的循环从花括号开始计深
            let mut depth = 0usize;
            while i < chars.len() {
                match chars[i] {
                    '{' => {
                        depth += 1;
                        out.push('{');
                    }
                    '}' => {
                        depth -= 1;
                        out.push('}');
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    None
}

/// 把一条路径模板归一成可比较的形状。
///
/// - `${buildQuery(…)}` → 丢掉（查询串不属于路径）；
/// - 其余 `${…}` → `{}`（TS 的路径参数）；
/// - `{id}` / `{envId}` / `{projectId}` → `{}`（axum 的路径参数）。
pub(crate) fn normalize_path(template: &str) -> String {
    let chars = chars_of(template);
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && chars.get(i + 1) == Some(&'{') {
            let mut inner = String::new();
            let mut depth = 0usize;
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '{' => {
                        depth += 1;
                        if depth > 1 {
                            inner.push('{');
                        }
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        inner.push('}');
                    }
                    other => inner.push(other),
                }
                j += 1;
            }
            if !inner.contains("buildQuery") {
                out.push_str("{}");
            }
            i = j + 1;
            continue;
        }
        if chars[i] == '{' {
            if let Some(close) = chars[i..].iter().position(|&c| c == '}') {
                out.push_str("{}");
                i += close + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `commands.ts` 的 `COMMANDS`：camelCase 键 → snake_case 命令名。
fn command_map() -> BTreeMap<String, String> {
    let body = between(COMMANDS_TS, "export const COMMANDS = {", "} as const;");
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('/') {
                return None;
            }
            let (key, rest) = line.split_once(": '")?;
            let (value, _) = rest.split_once('\'')?;
            let looks_like_command = value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            looks_like_command.then(|| (key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// `commands.ts` 的命令名集合 —— 也就是 `generate_handler!` 该有的名字。
fn commands() -> BTreeSet<String> {
    command_map().into_values().collect()
}

/// `web-routes.ts` 的 `WEB_ROUTES`：命令名 → (方法, 归一后的路径)。
///
/// 表里的键是 camelCase 的 `COMMANDS` 属性名，故先经 `command_map()` 换成命令名再比。
fn web_routes() -> BTreeMap<String, Route> {
    let names = command_map();
    let body = between(WEB_ROUTES_TS, "export const WEB_ROUTES", "\n};");
    let chars = chars_of(body);
    let mark = "[COMMANDS.";
    let marks: Vec<usize> = (0..chars.len())
        .filter(|&i| chars[i..].starts_with(&chars_of(mark)))
        .collect();
    let mut out = BTreeMap::new();
    for (idx, &start) in marks.iter().enumerate() {
        let end = marks.get(idx + 1).copied().unwrap_or(chars.len());
        let chunk = &chars[start..end];
        let ident = {
            let after = find_seq(chunk, mark, 0).expect("块首就是标记本身") + mark.len();
            let close = find_seq(chunk, "]", after).expect("`[COMMANDS.` 后面必须有 `]`");
            chunk[after..close].iter().collect::<String>()
        };
        let key = names
            .get(&ident)
            .unwrap_or_else(|| panic!("WEB_ROUTES 引用了 COMMANDS 里没有的键 `{ident}`"))
            .clone();

        let method_at = find_seq(chunk, "method:", 0).expect("每个条目都有 `method:`");
        let (method, _) = string_at(chunk, method_at)
            .unwrap_or_else(|| panic!("{key} 的 `method:` 后面没有字符串"));
        let path_at = find_seq(chunk, "path:", 0).expect("每个条目都有 `path:`");
        let (path, _) =
            string_at(chunk, path_at).unwrap_or_else(|| panic!("{key} 的 `path:` 后面没有字符串"));

        out.insert(key, (method.to_uppercase(), normalize_path(&path)));
    }
    out
}

/// `src-tauri/src/lib.rs` 的 `generate_handler![…]` 里出现的命令名。
fn generate_handler() -> BTreeSet<String> {
    let body = between(DESKTOP_LIB_RS, "tauri::generate_handler![", "])");
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                return None;
            }
            let entry = line.strip_suffix(',')?.trim();
            let name = entry.rsplit("::").next()?.trim();
            let looks_like_command = name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            (!name.is_empty() && looks_like_command).then(|| name.to_string())
        })
        .collect()
}

/// 定义**住在 `src-tauri/src/lib.rs` 自己身上**的 `#[tauri::command]` 函数名。
fn lib_local_handlers() -> BTreeSet<String> {
    let chars = chars_of(DESKTOP_LIB_RS);
    let mut out = BTreeSet::new();
    let mut from = 0usize;
    while let Some(at) = find_seq(&chars, "#[tauri::command]", from) {
        let fn_at = match find_seq(&chars, "fn ", at) {
            Some(i) => i,
            None => break,
        };
        let name_start = fn_at + "fn ".len();
        let mut end = name_start;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        out.insert(chars[name_start..end].iter().collect::<String>());
        from = end;
    }
    out
}

/// `router.rs` 里 `.route(…)` 注册的全部 (方法, 归一后路径)。
///
/// `format!("{prefix}/")` 那条是**动态拼接**：前缀来自 `base_path`，不是固定路径，
/// 解析时跳过（它只在设置了反代前缀时才存在，且只是 SPA shell，不是 API 面）。
pub(crate) fn axum_routes() -> BTreeSet<Route> {
    let chars = chars_of(ROUTER_RS);
    let mut out = BTreeSet::new();
    let mut from = 0usize;
    while let Some(at) = find_seq(&chars, ".route(", from) {
        let open = at + ".route".len();
        let Some((arg, after)) = paren_arg(&chars, open) else {
            break;
        };
        from = after;
        let arg_chars = chars_of(&arg);
        let mut i = 0;
        while i < arg_chars.len() && arg_chars[i].is_whitespace() {
            i += 1;
        }
        if arg_chars.get(i) != Some(&'"') {
            continue; // `&format!(…)`：动态路径，跳过
        }
        let Some((path, after_path)) = string_at(&arg_chars, i) else {
            continue;
        };
        let chain: String = arg_chars[after_path..].iter().collect();
        for method in methods_in(&chain) {
            out.insert((method, normalize_path(&path)));
        }
    }
    out
}

/// `open` 指向 `(`；返回括号内的文本与收尾 `)` 之后的下标。
fn paren_arg(chars: &[char], open: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut i = open;
    let mut out = String::new();
    while i < chars.len() {
        match chars[i] {
            '(' => {
                depth += 1;
                if depth > 1 {
                    out.push('(');
                }
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((out, i + 1));
                }
                out.push(')');
            }
            other => out.push(other),
        }
        i += 1;
    }
    None
}

/// 方法链里出现的方法名（`get(a).post(b).layer(…)` → GET, POST），统一大写。
fn methods_in(chain: &str) -> Vec<String> {
    const METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];
    let chars = chars_of(chain);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let starts_word = chars[i].is_ascii_alphabetic()
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'));
        if !starts_word {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        let word: String = chars[i..end].iter().collect();
        if METHODS.contains(&word.as_str()) && chars.get(end) == Some(&'(') {
            out.push(word.to_uppercase());
        }
        i = end;
    }
    out
}

// ----- 断言 ---------------------------------------------------------------

fn describe<T: Ord + std::fmt::Debug>(actual: &BTreeSet<T>, expected: &BTreeSet<T>) -> String {
    let missing: Vec<&T> = expected.difference(actual).collect();
    let extra: Vec<&T> = actual.difference(expected).collect();
    format!("少了 {missing:?}，多了 {extra:?}")
}

fn assert_same_set<T: Ord + std::fmt::Debug>(
    actual: &BTreeSet<T>,
    expected: &BTreeSet<T>,
    what: &str,
) {
    assert!(
        actual == expected,
        "{what} 的差集不是空集：{}",
        describe(actual, expected)
    );
}

/// 文档化的 serve-web 例外：这两条命令只有浏览器形态有（桌面由 updater 插件装安装包），
/// 故 `generate_handler!` 里没有它们的处理函数是**对的**，不是漏注册。
const WEB_ONLY_COMMANDS: &[&str] = &["update_deployment", "update_apply"];

/// 文档化的桌面例外：这两条只有桌面壳有 —— 浏览器形态把日志交给浏览器下载（镜像里的数据
/// 目录属于服务端，没有「写进本机日志目录再打开文件管理器」这件事），链接则走浏览器自己的
/// `target="_blank"` 而不必借系统命令。故 `WEB_ROUTES` 里没有对应条目是**对的**，不是漏登记。
const DESKTOP_ONLY_COMMANDS: &[&str] = &["log_export_file", "open_external_url"];

/// `http_request` 的 `#[tauri::command]` 定义住在 `src-tauri/src/lib.rs` 自己身上（不在
/// `commands/` 目录里）。它的**命令名仍然在** `COMMANDS` 表（前端键 `httpRequest`），
/// 所以它不影响任何差集 —— 这里钉的是「只有它一个住在 lib.rs」这件事。
const LIB_LOCAL_HANDLERS: &[&str] = &["http_request"];

/// 只在 REST 侧存在的路由：前四条是给外部客户端与 MCP 的同族面（`GET` 单条读取、
/// 一次落一整份规格），在 OpenAPI 表里有条目；后两条是健康探针的两种写法，前端从不调用。
fn rest_only() -> BTreeSet<Route> {
    [
        // 外部客户端按 id 读一个项目 / 一个节点（前端只在树里拿整批，故不需要单条）。
        ("GET", "/api/projects/{}"),
        ("GET", "/api/nodes/{}"),
        // 端点详情整行的单条读（前端读的是 `endpoint_details_list` 的整批）。
        ("GET", "/api/nodes/{}/endpoint-detail"),
        // MCP 工具 `spec_upsert` 的同族 REST 面：内部 UI 不走它。
        ("POST", "/api/spec/upsert"),
        // 健康探针：探测方是 supervisor / 运维，不是 SPA。
        ("GET", "/api/healthz"),
        ("GET", "/healthz"),
    ]
    .into_iter()
    .map(|(method, path)| (method.to_string(), path.to_string()))
    .collect()
}

#[test]
fn every_command_has_a_web_route_and_no_route_lacks_a_command() {
    let commands = commands();
    for name in DESKTOP_ONLY_COMMANDS {
        assert!(
            commands.contains(*name),
            "`{name}` 列在 DESKTOP_ONLY_COMMANDS 里，却不在 COMMANDS 表里 —— 例外表已过期"
        );
    }

    let desktop_only: BTreeSet<String> = DESKTOP_ONLY_COMMANDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let expected: BTreeSet<String> = commands.difference(&desktop_only).cloned().collect();
    let routes: BTreeSet<String> = web_routes().into_keys().collect();
    assert_same_set(&routes, &expected, "WEB_ROUTES ↔ COMMANDS − DESKTOP_ONLY");
}

#[test]
fn the_desktop_handler_list_is_exactly_the_commands_minus_the_web_only_pair() {
    let commands = commands();
    let handlers = generate_handler();
    let web_only: BTreeSet<String> = WEB_ONLY_COMMANDS.iter().map(|s| s.to_string()).collect();
    let expected: BTreeSet<String> = commands.difference(&web_only).cloned().collect();

    // 两个方向一起比：少一条是「点了没反应」，多一条是没人调得到的死处理函数。
    assert_same_set(
        &handlers,
        &expected,
        "generate_handler ↔ COMMANDS − WEB_ONLY",
    );
}

#[test]
fn the_only_handler_defined_in_the_desktop_crate_root_is_http_request() {
    let local = lib_local_handlers();
    let expected: BTreeSet<String> = LIB_LOCAL_HANDLERS.iter().map(|s| s.to_string()).collect();
    assert_same_set(&local, &expected, "lib.rs 里自带的处理函数");

    // 它虽然住在 lib.rs，仍必须在前端表里有名字，否则就是调不到的死命令。
    let commands = commands();
    for name in &local {
        assert!(
            commands.contains(name),
            "`{name}` 住在 src-tauri/src/lib.rs，却不在 COMMANDS 表里"
        );
    }
}

#[test]
fn every_web_route_exists_in_the_axum_router() {
    let router = axum_routes();
    let mut missing = Vec::new();
    for (key, route) in web_routes() {
        if !router.contains(&route) {
            missing.push(format!("{key} → {} {}", route.0, route.1));
        }
    }
    assert!(
        missing.is_empty(),
        "这些 WEB_ROUTES 条目在 router.rs 里没有对应的 .route(…)：{missing:?}"
    );
}

#[test]
fn the_router_publishes_exactly_the_web_routes_plus_the_documented_rest_only_face() {
    let router = axum_routes();
    let web: BTreeSet<Route> = web_routes().into_values().collect();

    // 只比 API 面：`/` 与 `format!("{prefix}/")` 是 SPA shell，不是 API。
    let api_face: BTreeSet<Route> = router
        .into_iter()
        .filter(|(_, path)| path.starts_with("/api/") || path == "/healthz")
        .collect();

    let undocumented: BTreeSet<Route> = api_face.difference(&web).cloned().collect();
    assert_same_set(&undocumented, &rest_only(), "router.rs − WEB_ROUTES");
}
