//! base URL 解析、代理前缀替换、拼接与斜杠收拢 —— 对应 `src/lib/url.ts`。
//!
//! 两边必须一致，否则屏幕上的预览与真正发出去的请求描述的是两个 URL。

use simple_post_core::domain::{EndpointDetail, Environment};
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

/// 解析出来的一组值：后端 api 地址、实际访问地址与来源。
#[derive(Debug, Clone)]
pub struct ResolvedBaseUrl {
    /// 后端 api 地址——模块在环境里那一格的 `url`。
    pub base_url: String,
    /// 实际访问地址——那一格的 `proxy`。`None` = 没有代理，直接用 `base_url`。
    pub proxy: Option<String>,
    /// 这个结果是从哪来的。
    pub source: BaseUrlSource,
}

/// 算出端点最终生效的 base URL。
///
/// 优先级（从高到低）：
///   1. `endpoint.base_url_override` —— 每端点的自定义模式。`None` 落到模块那一格；
///      `Some(host)` 固定该 host；`Some("")` 表示「完全没有前缀」—— 端点处于自定义
///      模式，URL 字段里已经是完整地址。
///   2. `env.module_base_urls[module_id]` —— 环境 × 模块那一格。它的 `url` 是后端
///      api 地址，`proxy` 是实际访问地址。
///   3. `""` —— 什么都没有。
///
/// **没有回退层**：格子里没有值就是没有地址，不借用别的模块的格子。格子缺失是
/// 一个真实答案，不是一个洞。
///
/// `env.module_base_urls` 按 `module_id` 建键。环境取自 `detail.environment_id`；
/// 未选环境（`None`）时直接短路成空。
pub fn resolve_base_url(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    module_id: Option<&str>,
) -> ResolvedBaseUrl {
    // `Some("")` 是明确选择而不是缺省：端点处于自定义模式，URL 字段里就是完整地址，
    // 再前置环境的 base URL 会把它弄坏。与 src/lib/url.ts 的
    // `resolveEndpointBaseUrl` 对应 —— 两边不一致，前端预览与后端请求说的就是两个 URL。
    if let Some(ovr) = detail.base_url_override.as_deref() {
        return ResolvedBaseUrl {
            base_url: ovr.to_string(),
            proxy: None,
            source: BaseUrlSource::Override,
        };
    }

    // 没有模块就没有格子，模块的地址不是一个可以被借用的东西。
    let Some(module_id) = module_id else {
        return ResolvedBaseUrl {
            base_url: String::new(),
            proxy: None,
            source: BaseUrlSource::None,
        };
    };

    // 模块地址只存在 `env.module_base_urls[module_id]` 这一格里。缺失或为空表示这一格
    // （环境 × 模块）没配，与「什么都没点名」是同一个答案：`source: none`。这一格不向
    // 别的模块的格子回落 —— 空的格子是一个真实答案，不是一个洞。
    let cell = env.and_then(|e| e.module_base_urls.get(module_id));
    match cell {
        Some(cell) if !cell.url.is_empty() => ResolvedBaseUrl {
            base_url: cell.url.clone(),
            proxy: cell.proxy.clone(),
            source: BaseUrlSource::Module,
        },
        _ => ResolvedBaseUrl {
            base_url: String::new(),
            proxy: None,
            source: BaseUrlSource::None,
        },
    }
}

/// 用代理地址替换请求地址里的模块地址前缀。
///
/// 接口经代理网关转代时，后端 api 地址与实际访问地址不一致：模块那一格写的是
/// `http://svc.internal:8080/api/a`（后端 api 地址），而真正要打的是
/// `https://gw.corp.com/api/xxxx/a`（实际访问地址）。本函数把请求地址开头的那段
/// 模块地址换成代理地址，其余部分原样保留：
///
///   apply_proxy("http://svc.internal:8080/api/a", Some("https://gw.corp.com/api/xxxx/a"),
///               "http://svc.internal:8080/api/a/orders/list")
///     → "https://gw.corp.com/api/xxxx/a/orders/list"
///
/// 规则：
///
/// - `proxy` 为空或只有空白 → 原样返回。
/// - 前缀是 `base_url` 去掉尾部所有 `/`；前缀为空 → 原样返回。
/// - 请求地址以该前缀开头，**且**前缀之后紧跟的那一位是 `/`、`?` 或 `#`（或请求地址
///   恰好等于该前缀）→ 替换。边界这一条是必需的：没有它，`http://h/api` 会命中
///   `http://h/apixyz`。
/// - 其余情况原样返回：请求打的是另一个主机，或那一格压根没有地址。
///
/// 与 `src/lib/url.ts` 的 `applyProxy` 对应；两边必须一致，否则预览与真正发出的请求
/// 描述的是两个 URL。
pub fn apply_proxy(base_url: &str, proxy: Option<&str>, url: &str) -> String {
    let Some(proxy) = proxy else {
        return url.to_string();
    };
    let proxy = proxy.trim();
    if proxy.is_empty() {
        return url.to_string();
    }
    let prefix = base_url.trim_end_matches('/');
    if prefix.is_empty() {
        return url.to_string();
    }
    let Some(rest) = url.strip_prefix(prefix) else {
        return url.to_string();
    };
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('?') || rest.starts_with('#'))
    {
        return url.to_string();
    }
    format!("{proxy}{rest}")
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
///   - 未解析的 `{{var}}` 是文本不是分隔符，它该进 `missing_vars` 而不是被当作多余字符删掉
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
