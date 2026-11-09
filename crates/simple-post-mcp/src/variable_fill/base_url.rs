//! base URL 解析、路径前缀改写、拼接与斜杠收拢 —— 对应 `src/lib/url.ts`。
//!
//! 两边必须一致，否则屏幕上的预览与真正发出去的请求描述的是两个 URL。

use simple_post_core::domain::{EndpointDetail, Environment, RewriteRule};
// -------------------------------------------------------------------------
// base URL 解析 —— 对应 src/lib/url.ts::resolveEndpointBaseUrl
// -------------------------------------------------------------------------

/// 最终 baseUrl 的来源。用来给选择器一类的界面标注当前选项（「跟随模块」/「自定义」）。
#[derive(Debug, Clone, PartialEq)]
pub enum BaseUrlSource {
    /// `detail.base_url_override` 胜出。
    Override,
    /// 环境里「模块 × 环境」那一格。
    Module,
    /// 谁都没匹配上；此时返回什么由调用方决定（通常是 `""`）。
    None,
}

/// 解析出来的一组值：模块地址、路径前缀改写规则与来源。
#[derive(Debug, Clone)]
pub struct ResolvedBaseUrl {
    /// 模块地址——模块在环境里那一格的 `url`。
    pub base_url: String,
    /// 路径前缀改写规则——那一格的 `rewrite`。`None` = 没有改写。
    pub rewrite: Option<RewriteRule>,
    /// 这个结果是从哪来的。
    pub source: BaseUrlSource,
}

/// 算出端点最终生效的模块地址与路径前缀改写规则。
///
/// 地址的优先级（从高到低）：
///   1. `endpoint.base_url_override` —— 每端点的自定义模式。`None` 落到模块那一格；
///      `Some(host)` 固定该 host；`Some("")` 表示「完全没有前缀」—— 端点处于自定义模式，
///      URL 字段里已经是完整地址。
///   2. `env.module_base_urls[module_id]` —— 环境 × 模块那一格。它的 `url` 是模块地址。
///   3. `""` —— 什么都没有。
///
/// 路径前缀改写规则只作用在**端点自己的 URL** 上，与地址来源无关：端点自带自定义主机时，
/// 路径照样按那一格的规则改写——主机是端点自己的事，路径前缀是这一格的事。
///
/// **没有回退层**：格子里没有值就是没有地址，不借用别的模块的格子。格子缺失是一个真实答案，
/// 不是一个洞。
/// `env.module_base_urls` 按 `module_id` 建键。环境由调用方给出：端点行不再绑定环境，故它要么
/// 是这次调用点名的那一个，要么是项目的默认环境；两者都没有时（`None`）直接短路成空。
pub fn resolve_base_url(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    module_id: Option<&str>,
) -> ResolvedBaseUrl {
    // 那一格（环境 × 模块）里的配置：模块地址与路径前缀改写规则都在这里。没有模块就没有
    // 格子，模块的配置不是一个可以被借用的东西。
    let cell = module_id.and_then(|m| env.and_then(|e| e.module_base_urls.get(m)));
    let rewrite = cell.and_then(|c| c.rewrite.clone());

    // `Some("")` 是明确选择而不是缺省：端点处于自定义模式，URL 字段里就是完整地址，
    // 再前置环境的 base URL 会把它弄坏。与 src/lib/url.ts 的
    // `resolveEndpointBaseUrl` 对应 —— 两边不一致，前端预览与后端请求说的就是两个 URL。
    if let Some(ovr) = detail.base_url_override.as_deref() {
        return ResolvedBaseUrl {
            base_url: ovr.to_string(),
            rewrite,
            source: BaseUrlSource::Override,
        };
    }

    // 模块地址只存在 `env.module_base_urls[module_id]` 这一格里。缺失或为空表示这一格
    // （环境 × 模块）没配，与「什么都没点名」是同一个答案：`source: none`。这一格不向
    // 别的模块的格子回落 —— 空的格子是一个真实答案，不是一个洞。
    match cell {
        Some(cell) if !cell.url.is_empty() => ResolvedBaseUrl {
            base_url: cell.url.clone(),
            rewrite,
            source: BaseUrlSource::Module,
        },
        _ => ResolvedBaseUrl {
            base_url: String::new(),
            rewrite,
            source: BaseUrlSource::None,
        },
    }
}

/// 把端点**自己 URL** 的路径前缀从 `from` 换成 `to`。
///
/// 实现住在 `simple_post_core::url::apply_rewrite`：核心库那一层是中立位置，MCP 侧与导入
/// 导出侧都要用它，三处各写一份就会分叉。
///
///   apply_rewrite("/api/air/xxxxx",
///                 Some(&RewriteRule { from: "/api/air".into(), to: "/api/project-a/air".into() }))
///     → "/api/project-a/air/xxxxx"
///
/// 与 `src/lib/url.ts` 的 `applyRewrite` 对应；两边必须一致，否则预览与真正发出的请求描述的
/// 是两个 URL。共享样本是 `fixtures/base-url-parity.json`。
pub fn apply_rewrite(url: &str, rule: Option<&RewriteRule>) -> String {
    simple_post_core::url::apply_rewrite(url, rule)
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

/// 前导 `scheme://` 的字节长度，没有则 `None`。[`has_scheme`] 与 [`collapse_url_slashes`] 共用它，
/// 两者才不会对「scheme 到哪结束」产生分歧——这条边界正是决定哪些斜杠属于协议、哪些是笔误的
/// 依据。实现在 `simple_post_core::url::scheme_prefix_len`。
fn scheme_prefix_len(s: &str) -> Option<usize> {
    simple_post_core::url::scheme_prefix_len(s)
}

/// 收掉 URL **路径**里连续的 `/`，其余字节一概不动。输入必须是已经拼接**并且**已经完成
/// `{{var}}` 替换的 URL。
///
/// 拼接必然发生在替换之前（环境自己的 base URL 里也可能有模板），所以 [`join_base_url`]
/// 无从知道 `{{var}}` 会展开成什么。它插入一个分隔符，随后替换把 `http://host:9200/{{var}}`
/// 变成 `http://host:9200//api`，对多数服务端那是另一条路由。只有替换之后真相才成立。
///
/// 作用范围刻意很窄 —— 无脑收连斜杠会毁掉真实 URL：
///   - `scheme://` 不是路径，`https://` 必须原样存活
///   - 无 scheme 的前导 `//` 是协议相对引用（`//cdn.example.com/x`），斜杠有意义
///   - `?query` / `#fragment` 是数据不是路径：参数里嵌套的 URL（`?redirect=https://x//y`）
///     必须原样穿过
///   - 未解析的 `{{var}}` 是文本不是分隔符，它该进 `missing_vars` 而不是被当作多余字符删掉
///
/// 与 `src/lib/url.ts` 的 `collapseUrlSlashes` 对应；两边必须一致，否则预览与真正发出的请求
/// 描述的是两个 URL。
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
