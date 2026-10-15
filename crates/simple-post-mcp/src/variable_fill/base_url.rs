//! base URL 解析、拼接与斜杠收拢 —— 对应 `src/lib/url.ts::resolveEndpointBaseUrl`。
//!
//! 两边必须一致，否则屏幕上的预览与真正发出去的请求描述的是两个 URL。

use simple_post_core::domain::{EndpointDetail, Environment, Project};
// -------------------------------------------------------------------------
// base URL 解析 —— 对应 src/lib/url.ts::resolveEndpointBaseUrl
// -------------------------------------------------------------------------

/// 最终 baseUrl 的来源。用来给选择器一类的界面标注当前选项（「模块默认」/「鉴权服务」
/// /「自定义」）；MCP daemon 用同一个 enum 把自己选了哪一路解释回渲染进程。
#[derive(Debug, Clone, PartialEq)]
pub enum BaseUrlSource {
    /// `detail.baseUrlOverride` 胜出。
    Override,
    /// 明确点名了某个服务 —— 端点自己的 `baseUrlDefinitionId`，或调用方的
    /// `forced_def_id`。
    Definition,
    /// 项目 ★（`Project.defaultBaseUrlDefinitionId`）胜出。
    Default,
    /// 谁都没匹配上；兜底由调用方负责（通常是 `""`）。
    None,
}

/// 给定提供 `moduleBaseUrls[defId][moduleId]` 的环境、以及声明服务的项目，算出端点
/// 最终生效的 baseUrl。
///
/// 优先级（从高到低）：
///   1. `endpoint.base_url_override` —— 每端点的自定义模式。`None` 落到服务链；
///      `Some(host)` 钉死该 host；`Some("")` 表示「完全没有前缀」—— 端点处于自定义
///      模式，URL 字段里已经是完整地址。
///   2. `detail.base_url_definition_id`，再退到项目 ★
///      （`project.default_base_url_definition_id`）。
///   3. `""` —— 什么都没有。
///
/// 这个三参数形式不带模块上下文，所以第 2 步没有格子可读，除每端点 override 外一律
/// 解析成 `""`。知道模块的调用方必须用 [`resolve_base_url_with`]。
///
/// `env.module_base_urls` 按 `BaseUrlDefinition.id` 再按模块 id 建键，**不按名字**：
/// 改名便宜，解析也确定 —— 即便两个服务同名。环境取自 `detail.environment_id`；
/// 未选环境（None）时直接短路成 ""。
pub fn resolve_base_url(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
) -> (String, BaseUrlSource, Option<String>) {
    resolve_base_url_with(detail, env, project, None, None)
}

/// [`resolve_base_url`] 再加上调用方指定的定义选择与模块。
///
/// `forced_def_id` 优先于 `detail.base_url_definition_id`，但仍让位给
/// `detail.base_url_override` —— 那是每端点的 host（或明确的「不要前缀」），始终是
/// 最强信号。`Some("")` 表示「没有 base URL」，渲染进程在用户清空选择器时就是这么
/// 发的。
///
/// `module_id` 选的是这次调用读**取值层的哪一格**，不是「哪些服务存在」：服务是项目
/// 级的，而 `env.module_base_urls[def_id][module_id]` 是 URL 唯一的住址。所以
/// `None` 解析成 `""`，而不是替别的模块猜一格。
///
/// 由调用方传入而非在此推导，是为了保持本函数纯净：模块住在 `nodes` 表里，而本模块
/// 刻意不认识 `nodes`。调用方经 [`EndpointOverrides::module_id`] 传下来。
///
/// 与 `src/lib/url.ts` 的 `resolveEndpointBaseUrl` 对应；两边必须一致，否则屏幕上的
/// 预览与真正发出去的请求描述的是两个 URL。
pub fn resolve_base_url_with(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
    forced_def_id: Option<&str>,
    module_id: Option<&str>,
) -> (String, BaseUrlSource, Option<String>) {
    // `Some("")` 是明确选择而不是缺省：端点处于自定义模式，URL 字段里就是完整地址，
    // 再前置环境的 base URL 会把它弄坏。与 src/lib/url.ts 的
    // `resolveEndpointBaseUrl` 对应 —— 两边不一致，前端预览与后端请求说的就是两个 URL。
    if let Some(ovr) = detail.base_url_override.as_deref() {
        return (ovr.to_string(), BaseUrlSource::Override, None);
    }
    let forced = forced_def_id.map(|s| s.to_string());
    if matches!(forced.as_deref(), Some("")) {
        return (String::new(), BaseUrlSource::None, None);
    }

    // 谁来应答：调用方的选择，否则端点自己的，否则项目 ★。端点没点名是常态，不是缺口 ——
    // ★ 就是为此存在的。
    let explicit = forced.or_else(|| detail.base_url_definition_id.clone());
    // 必须在下面的 match 消费掉 `explicit` 之前定下来：这里关心的是*哪一行应答*，
    // 不是哪个值，所以强制选择与端点自选都报 `Definition`。
    let was_explicit = explicit.is_some();
    let def_id = match explicit {
        Some(def_id) => def_id,
        None => match project.default_base_url_definition_id.clone() {
            Some(def_id) => def_id,
            // 没有 ★：端点就这么不带 base URL 发出，没有更上层可借，也没有服务可猜。
            None => return (String::new(), BaseUrlSource::None, None),
        },
    };
    // 这个 id 必须点名项目真实持有的服务。没命中可能是服务已删、id 从别的项目复制而来，
    // 或端点上的指针过期 —— 三种都表示「没有 base URL」，而不是「悄悄换个 host」。
    // 刻意**不**回退到 ★。
    if !project.base_url_definitions.iter().any(|d| d.id == def_id) {
        return (String::new(), BaseUrlSource::None, None);
    }

    // 没有模块就没有格子。服务由每个模块共享，地址不是 —— 无从可读。
    let module_id = match module_id {
        Some(module_id) => module_id,
        None => return (String::new(), BaseUrlSource::None, None),
    };

    // URL 唯一的住址。缺失或为空表示这一格 (环境 × 服务 × 模块) 没配，与「什么都没
    // 点名」是同一个答案：`source: none`。刻意既不回退到 `env.base_urls`（死列），
    // 也不借用别的模块的格子 —— 空格子是一个真实答案，不是一个洞。
    match env
        .and_then(|e| e.module_base_urls.get(&def_id))
        .and_then(|cells| cells.get(module_id))
    {
        Some(u) if !u.is_empty() => {
            let source = if was_explicit {
                BaseUrlSource::Definition
            } else {
                BaseUrlSource::Default
            };
            (u.clone(), source, Some(def_id))
        }
        _ => (String::new(), BaseUrlSource::None, None),
    }
}

/// 安全地拼接 `base_url` 与 `path`。`path` 自带 scheme 时按已是绝对地址处理，拼接处
/// 的重复斜杠也会被收掉。
///
///   join_base_url("https://api.example.com", "/users/1")
///     → "https://api.example.com/users/1"
///   join_base_url("https://api.example.com/", "users/1")
///     → "https://api.example.com/users/1"
///   join_base_url("", "/users/1")
///     → "/users/1"
///   join_base_url("https://api.example.com", "https://other/")
///     → "https://other/"  (path 胜出 —— 它自带 scheme)
pub fn join_base_url(base_url: &str, path: &str) -> String {
    if path.is_empty() {
        return base_url.to_string();
    }
    // 自带 scheme → path 完全自洽。
    if has_scheme(path) {
        return path.to_string();
    }
    let trimmed_base = base_url.trim_end_matches('/');
    if trimmed_base.is_empty() {
        return path.to_string();
    }
    if path.starts_with('/') {
        format!("{trimmed_base}{path}")
    } else {
        format!("{trimmed_base}/{path}")
    }
}

fn has_scheme(s: &str) -> bool {
    scheme_prefix_len(s).is_some()
}

/// 前导 `scheme://` 的字节长度，没有则 `None`。[`has_scheme`] 与
/// [`collapse_url_slashes`] 共用它，两者才不会对「scheme 到哪结束」产生分歧 ——
/// 这条边界正是决定哪些斜杠属于协议、哪些是笔误的依据。
fn scheme_prefix_len(s: &str) -> Option<usize> {
    // RFC 3986 scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )，对应 TS 正则
    // `/^[a-z][a-z0-9+.-]*:\/\//i`。
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    // 接下来要求 `:` 后跟 `//`。
    if i + 2 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b'/' && bytes[i + 2] == b'/' {
        Some(i + 3)
    } else {
        None
    }
}

/// 收掉 URL **路径**里连续的 `/`，其余字节一概不动。输入必须是已经拼接**并且**已经
/// 完成 `{{var}}` 替换的 URL。
///
/// 拼接必然发生在替换之前（环境自己的 base URL 里也可能有模板），所以
/// [`join_base_url`] 无从知道 `{{var}}` 会展开成什么。它插入一个分隔符，随后替换把
/// `http://host:9200/{{var}}` 变成 `http://host:9200//api`，对多数服务端那是另一条
/// 路由。只有替换之后真相才成立。
///
/// 作用范围刻意很窄 —— 无脑收连斜杠会毁掉真实 URL：
///   - `scheme://` 不是路径，`https://` 必须原样存活
///   - 无 scheme 的前导 `//` 是协议相对引用（`//cdn.example.com/x`），斜杠有意义
///   - `?query` / `#fragment` 是数据不是路径：参数里嵌套的 URL
///     （`?redirect=https://x//y`）必须原样穿过
///   - 未解析的 `{{var}}` 是文本不是分隔符，它该进 `missing_vars` 而不是被抹平
///
/// 与 `src/lib/url.ts` 的 `collapseUrlSlashes` 对应；两边必须一致，否则预览与真正发
/// 出的请求描述的是两个 URL。
pub fn collapse_url_slashes(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    // `scheme://` 与协议相对的 `//` 都是头部，不是路径。
    let (head, rest) = match scheme_prefix_len(url) {
        Some(n) => url.split_at(n),
        None if url.starts_with("//") => url.split_at(2),
        None => ("", url),
    };
    // authority 里不会有 `/`（到第一个 `/` 就结束），所以头部之后都可以当路径。
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let (path, tail) = rest.split_at(cut);

    let mut out = String::with_capacity(head.len() + path.len() + tail.len());
    out.push_str(head);
    let mut prev_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(ch);
    }
    out.push_str(tail);
    out
}
