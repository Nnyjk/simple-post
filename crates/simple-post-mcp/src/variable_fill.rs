//! 变量填充 + baseUrl 解析 + 鉴权注入。
//!
//! 对应渲染进程侧的 `src/lib/url.ts`（`resolveEndpointBaseUrl`、`resolveVars`）与
//! `src/lib/http-build.ts`（`buildFetchInit`）。逻辑放在这里，MCP daemon 与 Tauri
//! 命令两条路才共用同一个解析器，而不会各自漂移。
//!
//! 两层 API：
//!
//!   * [`resolve_template`] —— 对变量表做 `{{var}}` 替换，底层积木。
//!   * [`resolve_endpoint`] —— 把 [`EndpointDetail`] + 当前 [`Environment`] +
//!     [`Project`] + 可选 [`EndpointOverrides`] 合成为可直接交给发送器的
//!     [`ResolvedEndpoint`]。

use std::collections::HashMap;

use serde::Deserialize;

use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, EndpointDetail, Environment,
    HttpMethod, OAuth2Auth, Project, RequestBody,
};
use simple_post_core::error::AppError;

// -------------------------------------------------------------------------
// 公共类型
// -------------------------------------------------------------------------

/// `{{var}}` 替换的结果。`missing` 已去重并按出现顺序排列，调用方可以直接拿它渲染
/// 「未解析变量」列表。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTemplate {
    pub resolved: String,
    pub missing: Vec<String>,
}

/// 调用方在发送时给出的覆盖值。每个字段都是 `Option<T>`，于是「没覆盖」与「覆盖成
/// None / 空」是两回事。让渲染进程能做 curl 式的临时改动而不改写已存的端点。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOverrides {
    pub url: Option<String>,
    pub method: Option<HttpMethod>,
    pub headers: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub params: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub body: Option<RequestBody>,
    /// 覆盖端点上已存的 base URL 定义，改用这个去解析。
    ///
    /// 渲染进程的 base URL 选择器是临时的：用户不保存端点也能切换定义，所以发送时
    /// `detail.base_url_definition_id` 并不权威。没有这个字段，浏览器就得自己先拼
    /// base URL，「URL 组装搬到服务端」这件事就白做了。
    ///
    /// `None` = 用端点自己的值；`Some(id)` = 覆盖；空串 = 「没有 base URL」，
    /// 渲染进程用它表示「对着一条裸路径发请求」。
    pub base_url_definition_id: Option<String>,
    /// 该端点读的是 `env.moduleBaseUrls` 里哪个模块的**格子**。
    ///
    /// 服务属于项目而非模块，所以模块不是「哪些服务存在」的过滤器 —— 每个模块都看到
    /// 全量列表。它选的是取值：URL 住在 `(环境 × 服务 × 模块)`，两个模块可以让同一个
    /// 服务指向不同 host。
    ///
    /// 由调用方传入而不是在这里推导，因为本函数是纯的、没有 DB 句柄，而模块住在
    /// `nodes` 表里。`None` 表示「没有树上下文」：没有格子可读，只剩每端点的
    /// `base_url_override` 能产出 base URL。
    pub module_id: Option<String>,
}

/// `resolve_endpoint` 的产物：`{{var}}` 已全部替换、baseUrl 已拼好、鉴权配置已化到
/// 解析后的形态。HTTP 发送器（`simple_post_core::http_executor`）直接消费它。
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub params: Vec<(String, String)>,
    pub body: Option<String>,
    pub auth: AuthConfig,
    pub missing_vars: Vec<String>,
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

// -------------------------------------------------------------------------
// 鉴权 —— 对 bearer / basic / apikey 各字段做 `{{var}}` 替换
// -------------------------------------------------------------------------

/// 对鉴权配置里所有带用户可见文本的字段做 `{{var}}` 替换。返回**新的**
/// `AuthConfig`，不改传入值（调用方可能在多次发送之间复用同一个端点对象）。
/// 鉴权文本里缺的变量汇入 URL 扫描产出的同一份 missing 列表。
fn resolve_auth(auth: &AuthConfig, vars: &HashMap<String, String>) -> (AuthConfig, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record = |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            missing.push(var);
        }
    };

    let kind = auth.kind.clone();
    let bearer = match (kind.clone(), auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) => {
            let r = resolve_template(b, vars);
            for m in r.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(r.resolved)
        }
        _ => auth.bearer.clone(),
    };
    let basic = match (kind.clone(), auth.basic.as_ref()) {
        (AuthType::Basic, Some(b)) => {
            let u = resolve_template(&b.username, vars);
            for m in u.missing {
                record(&mut missing, &mut seen, m);
            }
            let p = resolve_template(&b.password, vars);
            for m in p.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(BasicAuth {
                username: u.resolved,
                password: p.resolved,
            })
        }
        _ => auth.basic.clone(),
    };
    let apikey = match (kind.clone(), auth.apikey.as_ref()) {
        (AuthType::Apikey, Some(a)) => {
            let k = resolve_template(&a.key, vars);
            for m in k.missing {
                record(&mut missing, &mut seen, m);
            }
            let v = resolve_template(&a.value, vars);
            for m in v.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(ApiKeyAuth {
                key: k.resolved,
                value: v.resolved,
                in_location: a.in_location,
            })
        }
        _ => auth.apikey.clone(),
    };
    let oauth2 = match (kind.clone(), auth.oauth2.as_ref()) {
        (AuthType::Oauth2, Some(o)) => Some(resolve_oauth2(o, vars, &mut missing, &mut seen)),
        _ => auth.oauth2.clone(),
    };
    (
        AuthConfig {
            kind,
            bearer,
            basic,
            apikey,
            oauth2,
        },
        missing,
    )
}

/// OAuth2 配置里的 `{{var}}` 替换。
///
/// **只替换 token URL / authorization URL / client id / scope 这类会被发出去的地址与
/// 标识**，刻意不碰 `client_secret`、`password`、`auth_code`：密钥写成 `{{var}}` 很常见
/// （环境变量表里放 secret），但解出来的值会顺着本函数的返回值一路进
/// `ResolvedEndpoint.auth`，而那是被 MCP 的响应回显读到的。少替换几个字段换来密钥
/// 不被解析层复制，是划算的——要按环境给 secret 就写进环境本身。
fn resolve_oauth2(
    o: &OAuth2Auth,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
    seen: &mut HashMap<String, ()>,
) -> OAuth2Auth {
    let resolve_text = |text: &str, missing: &mut Vec<String>, seen: &mut HashMap<String, ()>| {
        let r = resolve_template(text, vars);
        for m in r.missing {
            if seen.insert(m.clone(), ()).is_none() {
                missing.push(m);
            }
        }
        r.resolved
    };
    let mut out = o.clone();
    out.token_url = resolve_text(&o.token_url, missing, seen);
    out.authorization_url = resolve_text(&o.authorization_url, missing, seen);
    out.client_id = resolve_text(&o.client_id, missing, seen);
    out.scope = resolve_text(&o.scope, missing, seen);
    out
}

// -------------------------------------------------------------------------
// URL / header / param / body 组装
// -------------------------------------------------------------------------

/// 构建端点的完整有序请求头列表。
///
/// 顺序：用户请求头（启用且非空）→ body 模式蕴含时补的 `Content-Type`（缺失才补）
/// → 默认 `Accept: application/json` → 鉴权推出的 `Authorization`（或作为请求头的
/// apikey）。重复键按大小写不敏感判断（HTTP 请求头名本就不敏感）。
///
/// `apikey.inLocation == Query` 在这里**不**生成请求头 —— 由调用方经
/// `append_apikey_query` 追加到 URL。
fn build_headers(
    endpoint_headers: &[simple_post_core::domain::KeyValue],
    body_mode: &simple_post_core::domain::BodyMode,
    auth: &AuthConfig,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out: Vec<(String, String)> = Vec::new();

    let record_missing = |out: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            out.push(var);
        }
    };

    for h in endpoint_headers {
        if !h.enabled || h.key.is_empty() {
            continue;
        }
        let r = resolve_template(&h.value, vars);
        for m in r.missing {
            record_missing(missing, &mut seen, m);
        }
        out.push((h.key.clone(), r.resolved));
    }

    if let Some(ct) = auto_content_type(body_mode) {
        if !out
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            out.push(("Content-Type".to_string(), ct.to_string()));
        }
    }
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) {
        out.push(("Accept".to_string(), "application/json".to_string()));
    }

    match (&auth.kind, auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) if !b.is_empty() => {
            if !out
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            {
                out.push(("Authorization".to_string(), format!("Bearer {b}")));
            }
        }
        (AuthType::Basic, _) => {
            if let Some(b) = &auth.basic {
                if !out
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                {
                    let token = b64_basic(&b.username, &b.password);
                    out.push(("Authorization".to_string(), format!("Basic {token}")));
                }
            }
        }
        (AuthType::Apikey, _) => {
            if let Some(a) = &auth.apikey {
                if a.in_location == ApiKeyLocation::Header
                    && !out.iter().any(|(k, _)| k.eq_ignore_ascii_case(&a.key))
                {
                    out.push((a.key.clone(), a.value.clone()));
                }
            }
        }
        _ => {}
    }

    out
}

fn auto_content_type(mode: &simple_post_core::domain::BodyMode) -> Option<&'static str> {
    use simple_post_core::domain::BodyMode;
    match mode {
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Raw => Some("text/plain"),
        BodyMode::None => None,
    }
}

fn append_apikey_query(url: String, auth: &AuthConfig) -> String {
    if let AuthConfig {
        kind: AuthType::Apikey,
        apikey: Some(a),
        ..
    } = auth
    {
        if a.in_location == ApiKeyLocation::Query && !a.key.is_empty() {
            let glue = if url.contains('?') { '&' } else { '?' };
            return format!("{url}{glue}{}={}", url_encode(&a.key), url_encode(&a.value));
        }
    }
    url
}

fn append_query_string(mut url: String, params: &[(String, String)]) -> String {
    let mut first = !url.contains('?');
    for (k, v) in params {
        if k.is_empty() || v.is_empty() {
            continue;
        }
        let glue = if first { '?' } else { '&' };
        url.push(glue);
        url.push_str(&url_encode(k));
        url.push('=');
        url.push_str(&url_encode(v));
        first = false;
    }
    url
}

fn url_encode(s: &str) -> String {
    // 按 `application/x-www-form-urlencoded` 编码（与 TS 侧的 `encodeURIComponent`
    // 一致）：空格 → `+`，unreserved 集合之外一律 → 大写十六进制 `%XX`。
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 为 HTTP Basic 鉴权把 `user:pass` 做 base64。先过 UTF-8 字节，所以含非 ASCII 的
/// 凭据（中文用户名、emoji 密码）也编码正确 —— 与浏览器、以及 Express 等多数服务端
/// 的约定一致（把请求头当裸字节解码）。
fn b64_basic(user: &str, pass: &str) -> String {
    let combined = format!("{user}:{pass}");
    base64_encode(combined.as_bytes())
}

/// 极简 base64 编码器，标准字母表（`A-Z a-z 0-9 + /`）、`=` 补齐。只为
/// `Authorization: Basic` 而存在 —— 为此拉一个 `base64` crate 不如手写这 30 行。
fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

// -------------------------------------------------------------------------
// resolve_endpoint —— 主 API
// -------------------------------------------------------------------------

/// 把端点 + 环境 + 项目 + 覆盖值合成为完全解析好的线上形状。
///
/// 解析顺序（对应 `buildFetchInit`）：
///   1. 套用覆盖值（url / method / headers / params / body）
///   2. 经 [`resolve_base_url_with`] 解析 baseUrl
///   3. 到处替换 `{{var}}`（URL 路径、headers、params、body 内容、鉴权字段），然后
///      收掉路径里的重复斜杠（见 [`collapse_url_slashes`] —— 只有替换才会把拼接插入的
///      分隔符变成 `//`）
///   4. 追加启用的 query 参数（编码后）
///   5. 追加 apikey query（`auth.kind == Apikey && inLocation == Query` 时）
///   6. 构建有序请求头列表（用户头 + 自动 Content-Type + 自动 Accept + 鉴权推出的
///      `Authorization`）
///   7. 决定是否带上 body（仅当 `body.mode != None && content` 非空）
///
/// `missing_vars` 汇总所有没解析出来的 `{{x}}`，失败还是告警由调用方决定。
///
/// 模块走 `overrides.module_id` 而不是单独一个参数：它属于调用方上下文，与调用方自己
/// 选的 base URL 同类；放在覆盖值里，传入 `None` 的现有调用方都不用改，解析结果是
/// 「没有模块，也就没有格子可读」—— 这正是一个没有树上下文的端点该得的答案。
///
/// `AppError::Validation` 只用于明显畸形的输入。当前发送路径没有别的校验规则，真正
/// 用不了的时候由 HTTP 发送器自己报错。
pub fn resolve_endpoint(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
    overrides: Option<&EndpointOverrides>,
) -> Result<ResolvedEndpoint, AppError> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record_missing =
        |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
            if seen.insert(var.clone(), ()).is_none() {
                missing.push(var);
            }
        };

    // ---- 套用覆盖值（相关字段的浅拷贝）---------
    let mut eff_url = detail.url.clone();
    let mut eff_method = detail.method.clone();
    let mut eff_headers = detail.headers.clone();
    let mut eff_params = detail.params.clone();
    let mut eff_body = detail.body.clone();
    let eff_auth = detail.auth.clone();
    // 调用方的选择压过端点自带的定义 —— 渲染进程未保存的 base URL 选择正是靠这条生效。
    let eff_def_id = overrides.and_then(|o| o.base_url_definition_id.clone());
    if let Some(o) = overrides {
        if let Some(u) = o.url.clone() {
            eff_url = u;
        }
        if let Some(m) = o.method.clone() {
            eff_method = m;
        }
        if let Some(h) = o.headers.clone() {
            eff_headers = h;
        }
        if let Some(p) = o.params.clone() {
            eff_params = p;
        }
        if let Some(b) = o.body.clone() {
            eff_body = b;
        }
    }

    // ---- 变量池 ------------------------------------------------
    let vars: HashMap<String, String> = env.map(|e| e.variables.clone()).unwrap_or_default();

    // ---- params：先替换变量，再拼 URL。
    // 分两步做，missing 追踪与回显用的 params_out 列表才共用同一份真相。
    let mut params_subst: Vec<(String, String)> = Vec::with_capacity(eff_params.len());
    for p in &eff_params {
        if !p.enabled || p.key.is_empty() || p.value.is_empty() {
            continue;
        }
        let r = resolve_template(&p.value, &vars);
        for m in r.missing {
            record_missing(&mut missing, &mut seen, m);
        }
        params_subst.push((p.key.clone(), r.resolved));
    }

    // ---- baseUrl 解析 + URL 替换 ------------------------
    let (base_url, _src, _def_id) = resolve_base_url_with(
        detail,
        env,
        project,
        eff_def_id.as_deref(),
        overrides.and_then(|o| o.module_id.as_deref()),
    );
    let joined = join_base_url(&base_url, &eff_url);
    let url_r = resolve_template(&joined, &vars);
    for m in url_r.missing {
        record_missing(&mut missing, &mut seen, m);
    }
    // 替换之后、绝不之前：`{{var}}` → `/api` 才会把 `join_base_url` 插的分隔符变成
    // `//`，到这里真实路径才第一次已知。`collapse_url_slashes` 不碰 query/fragment
    // 数据，所以 params 仍能在它之后再追加。
    let url_norm = collapse_url_slashes(&url_r.resolved);

    // ---- 追加 query string（用已替换的 params）-------
    let url_with_params = append_query_string(url_norm, &params_subst);

    // ---- 鉴权解析 ---------------------------------------------
    let (resolved_auth, auth_miss) = resolve_auth(&eff_auth, &vars);
    for m in auth_miss {
        record_missing(&mut missing, &mut seen, m);
    }

    // ---- query 形式的 apikey ---------------------------------------------
    let final_url = append_apikey_query(url_with_params, &resolved_auth);

    // ---- body 是否带上 + 替换 --------------------------------
    let body = match &eff_body.mode {
        simple_post_core::domain::BodyMode::None => None,
        _ if eff_body.content.is_empty() => None,
        _ => {
            let b = resolve_template(&eff_body.content, &vars);
            for m in b.missing {
                record_missing(&mut missing, &mut seen, m);
            }
            Some(b.resolved)
        }
    };

    // ---- 请求头排序 ---------------------------------------------
    let headers = build_headers(
        &eff_headers,
        &eff_body.mode,
        &resolved_auth,
        &vars,
        &mut missing,
    );

    Ok(ResolvedEndpoint {
        url: final_url,
        method: eff_method.as_str().to_string(),
        headers,
        params: params_subst,
        body,
        auth: resolved_auth,
        missing_vars: missing,
    })
}

// -------------------------------------------------------------------------
// 测试
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "variable_fill_test.rs"]
mod tests;
