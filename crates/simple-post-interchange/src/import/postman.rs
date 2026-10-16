//! Postman Collection v2.1 → IR。
//!
//! 由 [`super::parse_text`] 分派进来；本模块只管 [`serde_json::Value`]。
//!
//! 三条纪律贯穿全文件：
//!
//! - **表达不了的构造必须出声。** 每一处降级映射都写一条 [`Diagnostic`]：静默丢一个
//!   字段与没写这个字段的解析器，对用户是同一件事。
//! - **路径是 path-only。** host 归 [`IrModule::servers`]，query 归
//!   [`IrEndpoint::params`]；拆不出 host 时整串留在 path 里——留一个错的路径，
//!   好过静默丢掉整条 URL。
//! - **嵌套深度无上限。** folder 树用显式工作栈遍历，故一棵畸形外来文件只会拿到一堆
//!   诊断，不会把预览进程带走。

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use simple_post_core::domain::{
    parse_http_method, uid, ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode,
    Environment, KeyValue, RequestBody,
};
use simple_post_core::error::AppError;
use simple_post_core::template::is_word;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrServer};

use super::ParsedExchange;

/// 一个 multipart 分片。与 `src/lib/multipart.ts` 的 `MultipartPart`、
/// `mcp/http_ops/multipart.rs` 同一套线格式。
///
/// `filename` / `contentType` 带 `skip_serializing_if`：写出的 `null` 会被前两者的
/// 逐字段校验判成非法输入，于是整个分片数组被丢成空数组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultipartPart {
    name: String,
    value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

/// 解析期的累积状态。
#[derive(Default)]
struct Ctx {
    diagnostics: Vec<Diagnostic>,
    /// 已报过的坏 `{{...}}` token（整串）。一份文件里同一个 token 只报一次：
    /// 同一个变量名出现十次不代表十个问题。
    seen_bad_tokens: HashSet<String>,
}

/// 一个条目在源文件里的形状。
enum Class<'a> {
    Request(&'a Map<String, Value>),
    Folder(&'a [Value]),
    Malformed,
}

/// 顶层散落请求共用的那个模块的 key。指「整份 `item[]`」这一个位置，与 OpenAPI 侧无 tag
/// 分组用的 `/paths` 同一套纪律：文档没给这批请求分组信息，而分组是要摆到用户面前的
/// 一层树，凭空按路径切一刀就是猜。
const LOOSE_KEY: &str = "/item";

/// 把一份 Postman v2.1 文档翻成 IR。
pub fn parse(value: &Value) -> Result<ParsedExchange, AppError> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Validation("Postman 集合的顶层必须是 JSON 对象".into()))?;

    let info = root.get("info").and_then(Value::as_object);
    let project_name = info
        .and_then(|i| field_text(i, "name"))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "未命名".to_string());
    let project_description = info.and_then(|i| description_text(i.get("description")));

    let mut ctx = Ctx::default();
    let top = root
        .get("item")
        .and_then(Value::as_array)
        .map(Vec::as_slice);
    let mut modules = Vec::new();

    // 顶层每个 item 各成一个模块。落进已有模块时它们会被降级成集合
    // （commit.rs 的 `CommitTarget::Module`），故「Postman 顶层 folder → 模块下的
    // 集合」不需要额外映射。
    // 顶层散落的请求（不是 folder 的 item）归到**同一个**模块里，名字取 `info.name`。
    // 每个顶层 item 各成一个模块的话，一份「请求平铺在根上」的集合会变成 N 个各含 1 个
    // 接口的集合 —— 与 OpenAPI 侧「无 tag 归到一个分组」是同一条路，见
    // `import/openapi.rs::group_of`。folder 仍各自成一个模块：Postman 的 folder 树
    // 是有意义的分组，不能压平。
    let mut loose_at: Option<usize> = None;

    if let Some(top) = top {
        for (i, item) in top.iter().enumerate() {
            let ptr = format!("/item/{i}");
            let Some(obj) = item.as_object() else {
                ctx.diagnostics
                    .push(Diagnostic::error(&ptr, "这一项不是 JSON 对象，已跳过"));
                continue;
            };

            let mut servers: Vec<IrServer> = Vec::new();
            match classify(obj, &ptr, &mut ctx) {
                Class::Request(req) => {
                    let mut endpoints = Vec::new();
                    if let Some(ep) =
                        build_endpoint(req, item_name(obj), &ptr, &mut servers, &mut ctx)
                    {
                        endpoints.push(ep);
                    }
                    if endpoints.is_empty() {
                        continue;
                    }
                    let at = match loose_at {
                        Some(at) => at,
                        None => {
                            modules.push(IrModule {
                                key: LOOSE_KEY.to_string(),
                                name: project_name.clone(),
                                description: None,
                                servers: Vec::new(),
                                endpoints: Vec::new(),
                                collections: Vec::new(),
                            });
                            modules.len() - 1
                        }
                    };
                    loose_at = Some(at);
                    modules[at].endpoints.extend(endpoints);
                    // 散落请求可能各带各的 host；按服务名去重后并进去。
                    for s in servers {
                        if !modules[at].servers.iter().any(|e| e.service == s.service) {
                            modules[at].servers.push(s);
                        }
                    }
                }
                Class::Folder(items) => {
                    let c = walk_children(items, &ptr, &mut servers, &mut ctx);
                    // 空容器不进树：预览里挂一个什么都没有的模块，只会让人以为少导了东西
                    // （落库侧同样跳过空容器，见 `ir::subtree_keys`）。
                    if c.endpoints.is_empty() && c.collections.is_empty() {
                        continue;
                    }
                    modules.push(IrModule {
                        key: ptr,
                        name: item_name(obj),
                        description: description_text(obj.get("description")),
                        servers,
                        endpoints: c.endpoints,
                        collections: c.collections,
                    });
                }
                Class::Malformed => continue,
            }
        }
    }

    let environments = parse_variables(root, &project_name);

    Ok(ParsedExchange {
        exchange: Exchange {
            project_name,
            project_description,
            environment_id: None,
            modules,
            diagnostics: ctx.diagnostics,
        },
        environments,
    })
}

// -------------------------------------------------------------------------
// 树遍历
// -------------------------------------------------------------------------

/// 一层容器的结果。
#[derive(Default)]
struct Container {
    endpoints: Vec<IrEndpoint>,
    collections: Vec<IrCollection>,
}

/// 显式工作栈的一层。
///
/// 递归遍历一棵深度无上限的 folder 树，会让畸形外来文件靠栈深度把进程带走；
/// 落库侧（`ir::endpoint_keys_of`）也是显式栈，两侧同一条纪律。
struct Frame<'a> {
    /// 本容器在源文件里的指针。子项是 `{key}/item/{i}`。
    key: String,
    name: String,
    description: Option<String>,
    /// 拥有本层的那一层的栈下标，`None` 为根层。
    ///
    /// 必须记下标而不是收尾时取 `last_mut()`：一个 folder 没走完时栈顶是**它自己**，
    /// 它的兄弟条目会被挂进这个 folder 里去。
    parent: Option<usize>,
    items: &'a [Value],
    idx: usize,
    endpoints: Vec<IrEndpoint>,
    collections: Vec<IrCollection>,
}

/// 遍历一层容器的全部子项，返回这一层的 endpoints 与 collections。
fn walk_children<'a>(
    items: &'a [Value],
    ptr: &str,
    servers: &mut Vec<IrServer>,
    ctx: &mut Ctx,
) -> Container {
    let mut stack: Vec<Frame<'a>> = vec![Frame {
        key: ptr.to_string(),
        name: String::new(),
        description: None,
        parent: None,
        items,
        idx: 0,
        endpoints: Vec::new(),
        collections: Vec::new(),
    }];

    loop {
        // 一次借用同时拿到下标与本层：`stack.len()` 与 `last_mut()` 分两次做会撞借检查。
        let Some((owner, top)) = stack.len().checked_sub(1).zip(stack.last_mut()) else {
            return Container::default();
        };
        if top.idx >= top.items.len() {
            let Some(done) = stack.pop() else {
                return Container::default();
            };
            let child = IrCollection {
                key: done.key,
                name: done.name,
                description: done.description,
                endpoints: done.endpoints,
                collections: done.collections,
            };
            match done.parent {
                Some(owner) => stack[owner].collections.push(child),
                None => {
                    return Container {
                        endpoints: child.endpoints,
                        collections: child.collections,
                    }
                }
            }
            continue;
        }

        let idx = top.idx;
        top.idx += 1;
        let items_ref: &'a [Value] = top.items;
        let parent_key = top.key.clone();
        let item = &items_ref[idx];
        let item_ptr = format!("{parent_key}/item/{idx}");

        let Some(obj) = item.as_object() else {
            ctx.diagnostics
                .push(Diagnostic::error(&item_ptr, "这一项不是 JSON 对象，已跳过"));
            continue;
        };

        match classify(obj, &item_ptr, ctx) {
            Class::Request(req) => {
                let name = item_name(obj);
                if let Some(ep) = build_endpoint(req, name, &item_ptr, servers, ctx) {
                    stack[owner].endpoints.push(ep);
                }
            }
            Class::Folder(children) => stack.push(Frame {
                key: item_ptr,
                name: item_name(obj),
                description: description_text(obj.get("description")),
                parent: Some(owner),
                items: children,
                idx: 0,
                endpoints: Vec::new(),
                collections: Vec::new(),
            }),
            Class::Malformed => {}
        }
    }
}

/// 一个条目是请求还是文件夹。
///
/// `request` 优先于 `item`：两者同时存在时（Postman 的 schema 允许「带请求的文件夹」）
/// 取请求，子项会丢，故出一条诊断。
fn classify<'a>(obj: &'a Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> Class<'a> {
    if let Some(req) = obj.get("request") {
        if let Some(arr) = obj
            .get("item")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        {
            ctx.diagnostics.push(
                Diagnostic::warn(ptr, "该请求条目下还挂着子项，子项不会被导入")
                    .dropping(format!("{} 个子项", arr.len())),
            );
        }
        return match req.as_object() {
            Some(m) => Class::Request(m),
            None => {
                ctx.diagnostics.push(Diagnostic::error(
                    format!("{ptr}/request"),
                    "request 不是 JSON 对象，已跳过这一条",
                ));
                Class::Malformed
            }
        };
    }

    match obj.get("item") {
        Some(Value::Array(arr)) => Class::Folder(arr),
        _ => {
            ctx.diagnostics.push(Diagnostic::error(
                ptr,
                "这一项既没有 request 也没有 item 数组，已跳过",
            ));
            Class::Malformed
        }
    }
}

// -------------------------------------------------------------------------
// endpoint
// -------------------------------------------------------------------------

fn build_endpoint(
    req: &Map<String, Value>,
    name: String,
    ptr: &str,
    servers: &mut Vec<IrServer>,
    ctx: &mut Ctx,
) -> Option<IrEndpoint> {
    let Some(url_value) = req.get("url") else {
        ctx.diagnostics.push(Diagnostic::error(
            format!("{ptr}/request"),
            "request 没有 url，已跳过这一条",
        ));
        return None;
    };

    let method_text = field_text(req, "method").unwrap_or_default();
    let method = match parse_http_method(&method_text) {
        Ok(m) => m,
        Err(e) => {
            ctx.diagnostics.push(
                Diagnostic::error(
                    format!("{ptr}/request/method"),
                    format!("method 不可用（{e}），已跳过这一条"),
                )
                .dropping(method_text),
            );
            return None;
        }
    };

    let url = UrlParts::new(url_value, &format!("{ptr}/request/url"), ctx);
    if let Some(server) = &url.server {
        // 同一 host 在这个模块里出现多次只算一个服务。
        if !servers.iter().any(|s| s.url == server.url) {
            servers.push(server.clone());
        }
    }

    let endpoint = IrEndpoint {
        key: format!("{ptr}/request"),
        name,
        description: description_text(req.get("description")),
        method,
        path: url.path,
        params: url.params,
        headers: kv_list(req.get("header"), &format!("{ptr}/request/header"), ctx),
        body: parse_body(req, &format!("{ptr}/request/body"), ctx),
        auth: parse_auth(req, &format!("{ptr}/request/auth"), ctx),
        tags: Vec::new(),
        ..Default::default()
    };
    scan_templates(&endpoint, ptr, ctx);
    Some(endpoint)
}

// -------------------------------------------------------------------------
// URL —— 拆成 path + params + 一台 server
// -------------------------------------------------------------------------

/// URL 的三段落法：path-only 的路径、query 参数、以及一台服务。
struct UrlParts {
    path: String,
    params: Vec<KeyValue>,
    server: Option<IrServer>,
}

impl UrlParts {
    fn new(value: &Value, ptr: &str, ctx: &mut Ctx) -> Self {
        // `url` 可以是整条字符串，也可以是拆开的对象；后者才带 query[] 的 disabled 位。
        let obj = value.as_object();
        let raw = match value {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => field_text(o, "raw")
                .filter(|s| !s.is_empty())
                .or_else(|| rebuild_raw(o)),
            _ => None,
        };

        let Some(raw) = raw.filter(|r| !r.is_empty()) else {
            return Self {
                path: String::new(),
                params: Vec::new(),
                server: None,
            };
        };

        let (protocol, authority, rest) = split_authority(&raw);
        let rest = rest.split('#').next().unwrap_or("");
        let (path_part, query) = match rest.split_once('?') {
            Some((p, q)) => (p, q),
            None => (rest, ""),
        };
        // 对象形态的 `query[]` 带 disabled 位，比从 raw 切出来的字符串更可信。
        let params = obj
            .and_then(query_params)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| parse_query_string(query));

        match authority {
            // 整体占位（`{{baseUrl}}/users`）拆不出稳定的服务名，整串留在 path 里。
            Some(host) if host.contains("{{") => {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        ptr,
                        "host 是一个整体变量，拆不出稳定的服务名；整串 URL 保留在 path 里",
                    )
                    .dropping(raw.clone()),
                );
                Self {
                    path: raw,
                    params,
                    server: None,
                }
            }
            Some(host) if !host.is_empty() => Self {
                path: path_part.to_string(),
                params,
                server: Some(IrServer {
                    service: super::service_name(host),
                    url: match protocol.filter(|p| !p.is_empty()) {
                        Some(p) => format!("{p}://{host}"),
                        None => format!("//{host}"),
                    },
                }),
            },
            _ => {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        ptr,
                        "URL 里没有 host（相对地址或整体变量），无法生成服务；整串保留在 path 里",
                    )
                    .dropping(raw.clone()),
                );
                Self {
                    path: path_part.to_string(),
                    params,
                    server: None,
                }
            }
        }
    }
}

/// `protocol` + `host` + `path` → 一条 raw，供 [`split_authority`] 统一处理。
///
/// 缺 protocol 时按原样拼（没有 `://`），后续自然落进「相对地址」分支并出诊断。
fn rebuild_raw(o: &Map<String, Value>) -> Option<String> {
    let host = match o.get("host") {
        Some(Value::Array(a)) => join_host(a),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let path = match o.get("path") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("/"),
        Some(Value::String(s)) => s.trim_start_matches('/').to_string(),
        _ => String::new(),
    };

    let mut raw = String::new();
    if let Some(p) = field_text(o, "protocol").filter(|p| !p.is_empty()) {
        raw.push_str(&p);
        raw.push_str("://");
    }
    raw.push_str(&host);
    if !path.is_empty() {
        if !raw.is_empty() && !host.is_empty() {
            raw.push('/');
        }
        raw.push_str(&path);
    }
    Some(raw)
}

/// host 数组拼回主机名。末位是纯数字时它其实是端口（`["localhost", "8080"]`），
/// 按域名用 `.` 连会得到一个不存在的主机。
fn join_host(arr: &[Value]) -> String {
    let parts: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
    match parts.split_last() {
        Some((last, head)) if !head.is_empty() && last.chars().all(|c| c.is_ascii_digit()) => {
            format!("{}:{}", head.join("."), last)
        }
        _ => parts.join("."),
    }
}

/// `(protocol, authority, 余下部分)`。没有 `://` 时 authority 为 `None`。
fn split_authority(raw: &str) -> (Option<&str>, Option<&str>, &str) {
    let Some(i) = raw.find("://") else {
        return (None, None, raw);
    };
    let rest = &raw[i + 3..];
    match rest.find(['/', '?']) {
        Some(j) => (Some(&raw[..i]), Some(&rest[..j]), &rest[j..]),
        None => (Some(&raw[..i]), Some(rest), ""),
    }
}

/// 对象形态的 `query[]`。
fn query_params(o: &Map<String, Value>) -> Option<Vec<KeyValue>> {
    let arr = o.get("query")?.as_array()?;
    Some(
        arr.iter()
            .map(|e| {
                let e = e.as_object();
                KeyValue {
                    key: e.and_then(|e| field_text(e, "key")).unwrap_or_default(),
                    value: e.and_then(|e| field_text(e, "value")).unwrap_or_default(),
                    enabled: !e.is_some_and(|e| flag(e, "disabled")),
                    ..Default::default()
                }
            })
            .collect(),
    )
}

/// raw 里切出来的 `a=1&b=2`。
fn parse_query_string(q: &str) -> Vec<KeyValue> {
    q.split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            KeyValue {
                key: k.to_string(),
                value: v.to_string(),
                enabled: true,
                ..Default::default()
            }
        })
        .collect()
}

// -------------------------------------------------------------------------
// body / auth
// -------------------------------------------------------------------------

fn parse_body(req: &Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> RequestBody {
    let Some(body) = req.get("body") else {
        return RequestBody::default();
    };
    let Some(b) = body.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(ptr, "body 不是 JSON 对象，按无 body 处理").dropping("body"));
        return RequestBody::default();
    };

    match field_text(b, "mode").unwrap_or_default().as_str() {
        "raw" => {
            let raw = field_text(b, "raw").unwrap_or_default();
            // Postman 的 raw 也可能本来就是 JSON 文本；language 缺失时按文本存。
            let language = b
                .get("options")
                .and_then(|o| o.get("raw"))
                .and_then(|r| r.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let mode = if language.eq_ignore_ascii_case("json") {
                BodyMode::Json
            } else {
                BodyMode::Raw
            };
            RequestBody {
                mode,
                content: raw,
                schema: None,
            }
        }
        "urlencoded" => {
            let parts = kv_list(b.get("urlencoded"), ptr, ctx);
            // 不做百分号编码：值里常有 `{{var}}`，编码后模板化就匹配不到了。
            let content = parts
                .iter()
                .filter(|p| p.enabled)
                .map(|p| format!("{}={}", p.key, p.value))
                .collect::<Vec<_>>()
                .join("&");
            RequestBody {
                mode: BodyMode::Form,
                content,
                schema: None,
            }
        }
        "formdata" => {
            let mut parts: Vec<MultipartPart> = Vec::new();
            if let Some(arr) = b.get("formdata").and_then(Value::as_array) {
                for (i, entry) in arr.iter().enumerate() {
                    let Some(e) = entry.as_object() else {
                        ctx.diagnostics.push(Diagnostic::warn(
                            format!("{ptr}/formdata/{i}"),
                            "这一项不是 JSON 对象，已跳过",
                        ));
                        continue;
                    };
                    let name = field_text(e, "key").unwrap_or_default();
                    if field_text(e, "type").as_deref().unwrap_or("text") == "file" {
                        ctx.diagnostics.push(
                            Diagnostic::warn(
                                format!("{ptr}/formdata/{i}"),
                                "multipart 的 file 字段无法导入（本仓库只存文本分片）",
                            )
                            .dropping(format!("multipart file 字段 {name}")),
                        );
                        continue;
                    }
                    // 分片结构没有 enabled 位，故被 Postman 禁用的文本字段照样落下。
                    parts.push(MultipartPart {
                        name,
                        value: field_text(e, "value").unwrap_or_default(),
                        filename: None,
                        content_type: None,
                    });
                }
            }
            RequestBody {
                mode: BodyMode::Multipart,
                content: serde_json::to_string(&parts).unwrap_or_default(),
                schema: None,
            }
        }
        "graphql" => {
            ctx.diagnostics.push(
                Diagnostic::warn(ptr, "graphql 请求体没有对应的标准形态，已丢弃")
                    .dropping("graphql 请求体"),
            );
            RequestBody::default()
        }
        "file" => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "二进制文件请求体无法导入").dropping("file 请求体"));
            RequestBody::default()
        }
        "" => RequestBody::default(),
        other => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "未知的 body mode，已按无 body 处理").dropping(other));
            RequestBody::default()
        }
    }
}

fn parse_auth(req: &Map<String, Value>, ptr: &str, ctx: &mut Ctx) -> AuthConfig {
    let Some(auth) = req.get("auth") else {
        return AuthConfig::default();
    };
    let Some(a) = auth.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(ptr, "auth 不是 JSON 对象，已按无 auth 处理").dropping("auth"));
        return AuthConfig::default();
    };
    let scheme = field_text(a, "type").unwrap_or_default();

    match scheme.as_str() {
        "noauth" | "" => AuthConfig::default(),
        "basic" => {
            let p = auth_params(a, "basic");
            AuthConfig {
                kind: AuthType::Basic,
                basic: Some(BasicAuth {
                    username: auth_param(&p, "username"),
                    password: auth_param(&p, "password"),
                }),
                ..Default::default()
            }
        }
        "bearer" => {
            let p = auth_params(a, "bearer");
            AuthConfig {
                kind: AuthType::Bearer,
                bearer: Some(auth_param(&p, "token")),
                ..Default::default()
            }
        }
        "apikey" => {
            let p = auth_params(a, "apikey");
            let location = match auth_param(&p, "in").as_str() {
                "query" => ApiKeyLocation::Query,
                "header" | "" => ApiKeyLocation::Header,
                other => {
                    // 只有 header / query 两种位置。猜成 header 会把凭据发到错的地方，
                    // 故整段丢弃并出声。
                    ctx.diagnostics.push(
                        Diagnostic::warn(ptr, "apikey 的位置无法表达，整段 auth 已丢弃")
                            .dropping(format!("apikey auth（in: {other}）")),
                    );
                    return AuthConfig::default();
                }
            };
            AuthConfig {
                kind: AuthType::Apikey,
                apikey: Some(ApiKeyAuth {
                    key: auth_param(&p, "key"),
                    value: auth_param(&p, "value"),
                    in_location: location,
                }),
                ..Default::default()
            }
        }
        other => {
            ctx.diagnostics
                .push(Diagnostic::warn(ptr, "未知的 auth 方案，已按无 auth 处理").dropping(other));
            AuthConfig::default()
        }
    }
}

/// Postman 的 auth 参数是 `[{key, value}]`；少数导出把它写成单个对象，两种都收。
fn auth_params(a: &Map<String, Value>, scheme: &str) -> Vec<(String, String)> {
    let read = |o: &Map<String, Value>| {
        (
            field_text(o, "key").unwrap_or_default(),
            field_text(o, "value").unwrap_or_default(),
        )
    };
    let mut out = Vec::new();
    match a.get(scheme) {
        Some(Value::Array(arr)) => {
            for e in arr.iter().filter_map(Value::as_object) {
                out.push(read(e));
            }
        }
        Some(Value::Object(o)) => out.push(read(o)),
        _ => {}
    }
    out
}

fn auth_param(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

// -------------------------------------------------------------------------
// 模板变量
// -------------------------------------------------------------------------

/// 扫一个 endpoint 所有落库字段里的 `{{...}}`。
///
/// 只认 `\w+`：其余两种形状都会被发送链路原样放过，不出声就是「导入成功、发送时不替换」。
fn scan_templates(ep: &IrEndpoint, ptr: &str, ctx: &mut Ctx) {
    let url_ptr = format!("{ptr}/request/url");
    let header_ptr = format!("{ptr}/request/header");
    let body_ptr = format!("{ptr}/request/body");
    let auth_ptr = format!("{ptr}/request/auth");

    scan_template(&ep.path, &url_ptr, ctx);
    for p in &ep.params {
        scan_template(&p.value, &url_ptr, ctx);
    }
    for h in &ep.headers {
        scan_template(&h.value, &header_ptr, ctx);
    }
    scan_template(&ep.body.content, &body_ptr, ctx);

    // `AuthConfig` 是「判别字段 + 可选载荷」的扁平结构，未选中方案的载荷同样为真，
    // 故只读 `kind` 选中的那一份。
    match ep.auth.kind {
        AuthType::Basic => {
            if let Some(b) = &ep.auth.basic {
                scan_template(&b.username, &auth_ptr, ctx);
                scan_template(&b.password, &auth_ptr, ctx);
            }
        }
        AuthType::Bearer => {
            if let Some(t) = &ep.auth.bearer {
                scan_template(t, &auth_ptr, ctx);
            }
        }
        AuthType::Apikey => {
            if let Some(a) = &ep.auth.apikey {
                scan_template(&a.key, &auth_ptr, ctx);
                scan_template(&a.value, &auth_ptr, ctx);
            }
        }
        AuthType::None | AuthType::Oauth2 => {}
    }
}

/// 一个字段里的 `{{...}}`。同一个坏 token 在一份文件里只报一次。
///
/// 标记名的判据走 [`is_word`]——与发送链路同一份，所以诊断说的话和实际行为不会分家。
fn scan_template(text: &str, ptr: &str, ctx: &mut Ctx) {
    if text.is_empty() {
        return;
    }
    // 扫的是 ASCII 的 `{}`；UTF-8 续字节都 ≥ 0x80，按字节推进不会切在字符中间。
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(text, i + 2) {
                let inner = &text[i + 2..i + 2 + end];
                let token = &text[i..i + 2 + end + 2];
                if !is_word(inner) && ctx.seen_bad_tokens.insert(token.to_string()) {
                    let d = if inner.starts_with('$') {
                        // Postman 动态变量在本仓库没有对应物，会原样留在 URL 里。
                        Diagnostic::info(
                            ptr,
                            "Postman 动态变量在本仓库没有对应物，发送时不会被替换",
                        )
                    } else {
                        Diagnostic::warn(
                            ptr,
                            "变量名含替换器不认的字符（只认字母数字与下划线），发送时不会被替换",
                        )
                    };
                    ctx.diagnostics.push(d.dropping(token));
                }
                i += 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
}

/// `}}` 相对 `from` 的偏移。
fn find_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i - from);
        }
        i += 1;
    }
    None
}

// -------------------------------------------------------------------------
// environment / 小工具
// -------------------------------------------------------------------------

/// 文档自带的 `variable[]` → 一个环境。
///
/// 它是**收集变量**，不是服务：导成 `base_url_definitions` 会凭空造出一台不存在的服务。
/// `is_active` 恒为 false：导入不抢激活位。
fn parse_variables(root: &Map<String, Value>, name: &str) -> Vec<Environment> {
    let Some(arr) = root.get("variable").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut variables = HashMap::new();
    for entry in arr {
        let Some(o) = entry.as_object() else { continue };
        let Some(k) = field_text(o, "key").filter(|k| !k.is_empty()) else {
            continue;
        };
        variables.insert(k, field_text(o, "value").unwrap_or_default());
    }
    if variables.is_empty() {
        return Vec::new();
    }
    vec![Environment {
        id: uid(),
        project_id: String::new(),
        name: name.to_string(),
        base_urls: HashMap::new(),
        module_base_urls: HashMap::new(),
        variables,
        is_active: false,
    }]
}

/// 条目名。名字是树里唯一能看见的东西，空着等于让用户对着一棵无名树。
fn item_name(obj: &Map<String, Value>) -> String {
    if let Some(n) = field_text(obj, "name").filter(|n| !n.trim().is_empty()) {
        return n;
    }
    obj.get("request")
        .and_then(raw_url_text)
        .unwrap_or_else(|| "未命名".to_string())
}

fn raw_url_text(request: &Value) -> Option<String> {
    match request.get("url") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Object(o)) => field_text(o, "raw")
            .filter(|s| !s.is_empty())
            .or_else(|| rebuild_raw(o)),
        _ => None,
    }
}

/// `[{key, value, disabled?}]` → [`KeyValue`]。非对象条目出诊断后跳过。
fn kv_list(value: Option<&Value>, ptr: &str, ctx: &mut Ctx) -> Vec<KeyValue> {
    let Some(Value::Array(arr)) = value else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let Some(o) = entry.as_object() else {
            ctx.diagnostics.push(Diagnostic::warn(
                format!("{ptr}/{i}"),
                "这一项不是 JSON 对象，已跳过",
            ));
            continue;
        };
        out.push(KeyValue {
            key: field_text(o, "key").unwrap_or_default(),
            value: field_text(o, "value").unwrap_or_default(),
            enabled: !flag(o, "disabled"),
            description: description_text(o.get("description")),
            ..Default::default()
        });
    }
    out
}

/// Postman 的值基本都是字符串；数字 / 布尔也收，其余形状当没有。
fn field_text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn flag(obj: &Map<String, Value>, key: &str) -> bool {
    matches!(obj.get(key), Some(Value::Bool(true)))
}

/// 描述可以是裸字符串，也可以是 `{content, type}`。
fn description_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => field_text(o, "content").filter(|s| !s.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "postman_test.rs"]
mod tests;
