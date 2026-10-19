//! OpenAPI 3.0 / 3.1 → IR。
//!
//! 由 [`super::parse_text`] 分派进来；本模块只管 [`serde_json::Value`]。YAML 在那之前
//! 已经落成 `Value`，故本模块没有第二套解析结构。
//!
//! 三条纪律贯穿全文件：
//!
//! - **表达不了的构造必须出声。** 每一处降级映射都写一条 [`Diagnostic`]。
//! - **`url` 只存 path。** host 归 [`IrModule::servers`]，`servers[0].url` 的 basePath
//!   前缀留在 path 上——多个模块各带各的前缀才说得通。
//! - **外来文档不受本仓库约束。** `$ref` 展开走 JSON Pointer，显式工作栈 + 访问栈 +
//!   深度上限，故一份畸形文档只会拿到一堆诊断，不会把预览进程带走。
//!
//! 结构兼容的 3.1 文档也收：联合类型数组（`["string","null"]`）按可选处理，`$ref`
//! 旁边的兄弟字段按覆盖处理。不做 JSON Schema 2020-12 求值，也不碰
//! `webhooks` / `jsonSchemaDialect` / `components.pathItems`。

use std::collections::HashMap;

use serde_json::{Map, Value};
use simple_post_core::domain::{
    parse_http_method, ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode,
    HttpMethod, KeyValue, RequestBody,
};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrEndpoint, IrModule, IrServer};

use super::ParsedExchange;

/// OpenAPI 3.x `paths` 的固定动词键集。`x-` 扩展键因为不在这个集合里而被自然跳过。
const PATH_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// `requestBody.content` 的偏好序：先到先得。`RequestBody` 只有一个 `content`，
/// 装不下第二个。
const CONTENT_PREFERENCE: [&str; 3] = [
    "application/json",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
];

/// `$ref` 展开的深度上限。没有它，一层套一层的 `$ref` 就能把导入器拖爆。
const MAX_REF_DEPTH: usize = 24;

/// 既没有 tag 也没有 `info.title` 时的名字。
const DEFAULT_NAME: &str = "默认";

/// 解析期的累积状态。
#[derive(Default)]
struct Ctx {
    diagnostics: Vec<Diagnostic>,
    /// 当前访问栈上的 `$ref` 指针。命中即成环。
    active: Vec<String>,
}

/// 逐个 operation 都要用到的东西，打包一次传下去。
struct DocView<'a> {
    root: &'a Value,
    title: Option<&'a str>,
    servers: &'a DocServers,
}

/// `servers[0].url` 拆完之后的样子。
struct DocServers {
    /// 文档级只有一个 host。URL 是 per (环境 × 模块 × 服务)，本仓库装不下多服务器。
    hosts: Vec<IrServer>,
    /// 留在 path 上的前缀。
    base_path: String,
}

/// 把一份 OpenAPI 文档翻成 IR。
pub fn parse(value: &Value) -> Result<ParsedExchange, AppError> {
    let root = value
        .as_object()
        .ok_or_else(|| AppError::Validation("OpenAPI 文档的顶层必须是 JSON 对象".into()))?;

    // 3.1 与 3.0 结构兼容，故两者都收；其它版本不猜。
    if let Some(declared) = root.get("openapi") {
        let version = declared.as_str().unwrap_or_default();
        if !version.starts_with("3.") {
            return Err(AppError::Validation(format!(
                "只处理 OpenAPI 3.x，这份文档声明的是 {version:?}"
            )));
        }
    }

    let title = root
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(Value::as_str);
    let Some(Value::Object(paths)) = root.get("paths") else {
        return Err(AppError::Validation(
            "OpenAPI 文档的 paths 不是对象，没有可导入的接口".into(),
        ));
    };

    let mut ctx = Ctx::default();
    let servers = doc_servers(root.get("servers"), &mut ctx);
    let view = DocView {
        root: value,
        title,
        servers: &servers,
    };

    let mut modules = Vec::new();
    for (raw_path, item) in paths {
        let pointer = format!("/paths/{}", escape(raw_path));
        let Some(item) = item.as_object() else {
            ctx.diagnostics.push(Diagnostic::error(
                &pointer,
                "path item 不是对象，这条路径被跳过",
            ));
            continue;
        };
        for verb in PATH_METHODS {
            let Some(operation) = item.get(verb) else {
                continue;
            };
            let op_pointer = format!("{pointer}/{verb}");
            let Some(operation) = operation.as_object() else {
                ctx.diagnostics.push(Diagnostic::error(
                    &op_pointer,
                    "operation 不是对象，这个操作被跳过",
                ));
                continue;
            };
            modules.push(operation_module(
                &view, raw_path, verb, operation, item, &mut ctx,
            ));
        }
    }

    Ok(ParsedExchange {
        exchange: Exchange {
            project_name: title.unwrap_or(DEFAULT_NAME).to_string(),
            project_description: root
                .get("info")
                .and_then(|i| i.get("description"))
                .and_then(Value::as_str)
                .map(str::to_string),
            environment_id: None,
            modules,
            diagnostics: ctx.diagnostics,
        },
        // OpenAPI 没有「环境」这一层，故这里永远是空的。
        environments: Vec::new(),
    })
}

/// 一个 operation 落成一个模块。落库时它会被降级成 collection。
fn operation_module(
    view: &DocView,
    raw_path: &str,
    verb: &str,
    operation: &Map<String, Value>,
    item: &Map<String, Value>,
    ctx: &mut Ctx,
) -> IrModule {
    let key = format!("/paths/{}/{}", escape(raw_path), verb);
    let path = format!("{}{raw_path}", view.servers.base_path);
    let tags = string_list(operation.get("tags"));

    // 固定动词键集里没有一个解析不了，故这里不需要回退。
    let method = parse_http_method(verb).unwrap_or(HttpMethod::Get);
    let name = text(operation, "summary")
        .or_else(|| text(operation, "operationId"))
        .unwrap_or_else(|| format!("{} {path}", verb.to_ascii_uppercase()));

    let (params, headers) = parameters(ctx, view, item, operation, &key);
    let body = request_body(ctx, view, operation, &key);
    let auth = auth_of(ctx, view, operation, &key);

    IrModule {
        key: key.clone(),
        name: tags
            .first()
            .cloned()
            .unwrap_or_else(|| view.title.unwrap_or(DEFAULT_NAME).to_string()),
        description: None,
        servers: view.servers.hosts.clone(),
        endpoints: vec![IrEndpoint {
            key,
            name,
            description: text(operation, "description"),
            method,
            path,
            params,
            headers,
            body,
            auth,
            docs: None,
            notes: None,
            tags,
        }],
        collections: Vec::new(),
    }
}

/// path-item 级 + operation 级的 `parameters`，后者覆盖前者。返回 (query, header)。
///
/// `in: path` 的参数已经在路径模板里，`in: cookie` 在 `ApiKeyLocation` 里没有对应变体，
/// 两者都出诊断后跳过。
fn parameters(
    ctx: &mut Ctx,
    view: &DocView,
    item: &Map<String, Value>,
    operation: &Map<String, Value>,
    pointer: &str,
) -> (Vec<KeyValue>, Vec<KeyValue>) {
    let merged = merged_parameters(ctx, view, item, operation, pointer);
    let mut params = Vec::new();
    let mut headers = Vec::new();
    for (i, value) in merged.iter().enumerate() {
        let at = format!("{pointer}/parameters/{i}");
        let Some(p) = value.as_object() else {
            ctx.diagnostics
                .push(Diagnostic::warn(&at, "这一项不是对象，已跳过").dropping(value.to_string()));
            continue;
        };
        let Some(name) = text(p, "name") else {
            ctx.diagnostics
                .push(Diagnostic::warn(&at, "参数没有 name，已跳过").dropping(value.to_string()));
            continue;
        };
        let at = format!("{at}/{}", escape(&name));
        // 只有显式 `required: false` 才禁用；3.1 的联合类型数组等价于可选。
        let enabled = !matches!(p.get("required"), Some(Value::Bool(false)));
        let default = p
            .get("schema")
            .and_then(|s| s.get("default"))
            .map(scalar_text)
            .unwrap_or_default();
        let description = text(p, "description");

        match text(p, "in").unwrap_or_default().as_str() {
            "query" => params.push(KeyValue {
                key: name,
                value: default,
                enabled,
                description,
                ..Default::default()
            }),
            "header" => headers.push(KeyValue {
                key: name,
                value: default,
                enabled,
                description,
                ..Default::default()
            }),
            "path" => ctx.diagnostics.push(Diagnostic::info(
                &at,
                "path 参数已经在路径模板里，不需要单独导入",
            )),
            "cookie" => ctx.diagnostics.push(
                Diagnostic::warn(
                    &at,
                    "ApiKeyLocation 只有 header 与 query，cookie 参数无法导入",
                )
                .dropping(format!("cookie {name}")),
            ),
            other => ctx.diagnostics.push(
                Diagnostic::warn(&at, format!("未知的参数位置 {other:?}，已跳过"))
                    .dropping(format!("{other} {name}")),
            ),
        }
    }
    (params, headers)
}

/// 两级 `parameters` 合并成一份，同名同 `in` 算同一个参数。
fn merged_parameters(
    ctx: &mut Ctx,
    view: &DocView,
    item: &Map<String, Value>,
    operation: &Map<String, Value>,
    pointer: &str,
) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for source in [item, operation] {
        let Some(raw) = source.get("parameters") else {
            continue;
        };
        let at_ptr = format!("{pointer}/parameters");
        let Some(Value::Array(entries)) = deref(ctx, view.root, raw, &at_ptr) else {
            continue;
        };
        for entry in entries {
            let key = param_identity(&entry);
            match at.get(&key) {
                Some(&slot) => out[slot] = entry,
                None => {
                    at.insert(key, out.len());
                    out.push(entry);
                }
            }
        }
    }
    out
}

/// 一个参数的身份。认不出身份（不是对象 / 没有 name）时退回成它自己的文本，
/// 否则这些条目会互相覆盖成一个。
fn param_identity(entry: &Value) -> String {
    match (entry.get("name"), entry.get("in")) {
        (Some(Value::String(name)), Some(Value::String(loc))) => format!("{loc}:{name}"),
        _ => format!("#{}", entry),
    }
}

/// `requestBody.content` 按偏好序取一个，其余 media type 逐个点名丢弃。
fn request_body(
    ctx: &mut Ctx,
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
) -> RequestBody {
    let Some(raw) = operation.get("requestBody") else {
        // 没有 requestBody 就是没有请求体，不是「待补」。
        return RequestBody::default();
    };
    let at_ptr = format!("{pointer}/requestBody");
    let Some(body) = deref(ctx, view.root, raw, &at_ptr) else {
        return RequestBody::default();
    };
    let Some(Value::Object(content)) = body.get("content") else {
        ctx.diagnostics.push(Diagnostic::warn(
            &at_ptr,
            "requestBody 没有 content，已按无请求体导入",
        ));
        return RequestBody::default();
    };

    let picked = CONTENT_PREFERENCE
        .iter()
        .find_map(|want| {
            content
                .keys()
                .find(|key| normalize_media(key) == *want)
                .cloned()
        })
        // 偏好序都没命中就取第一个，形态存进 Raw 而不是假装知道它是什么。
        .or_else(|| content.keys().next().cloned());

    let Some(picked) = picked else {
        ctx.diagnostics.push(Diagnostic::warn(
            format!("{at_ptr}/content"),
            "requestBody.content 是空的，已按无请求体导入",
        ));
        return RequestBody::default();
    };
    for key in content.keys() {
        if key != &picked {
            ctx.diagnostics.push(
                Diagnostic::warn(
                    format!("{at_ptr}/content/{}", escape(key)),
                    "一个请求体只装得下一种 media type，其余的无法导入",
                )
                .dropping(key.clone()),
            );
        }
    }

    let sample = content[&picked]
        .get("example")
        .or_else(|| content[&picked].get("default"));
    RequestBody {
        mode: mode_of(&normalize_media(&picked)),
        content: sample.map(scalar_text).unwrap_or_default(),
        schema: None,
    }
}

/// media type → [`BodyMode`]。`application/json` 与 `*+json` 都算 JSON。
fn mode_of(media: &str) -> BodyMode {
    match media {
        "application/x-www-form-urlencoded" => BodyMode::Form,
        "multipart/form-data" => BodyMode::Multipart,
        m if m == "application/json" || m.ends_with("+json") => BodyMode::Json,
        _ => BodyMode::Raw,
    }
}

/// operation 级 `security` 覆盖文档级。空数组是显式的「不要鉴权」。
fn auth_of(
    ctx: &mut Ctx,
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
) -> AuthConfig {
    let at = format!("{pointer}/security");
    let source = operation
        .get("security")
        .or_else(|| view.root.get("security"));
    let Some(Value::Array(requirements)) = source else {
        return AuthConfig::default();
    };
    let Some(Value::Object(first)) = requirements.first() else {
        return AuthConfig::default();
    };
    for name in first.keys() {
        if let Some(auth) = scheme_of(ctx, view, name, &at) {
            return auth;
        }
    }
    AuthConfig::default()
}

/// 一条 securityScheme → [`AuthConfig`]。映射不了返回 `None`，诊断已报在里面。
fn scheme_of(ctx: &mut Ctx, view: &DocView, name: &str, pointer: &str) -> Option<AuthConfig> {
    let at = format!("{pointer}/{}", escape(name));
    let raw = view
        .root
        .get("components")
        .and_then(|c| c.get("securitySchemes"))
        .and_then(|s| s.get(name))?;
    let scheme = deref(ctx, view.root, raw, &at)?;
    let Some(scheme) = scheme.as_object() else {
        ctx.diagnostics
            .push(Diagnostic::warn(&at, "securityScheme 不是对象，已跳过"));
        return None;
    };

    match text(scheme, "type").unwrap_or_default().as_str() {
        "http" => {
            let http = text(scheme, "scheme")
                .unwrap_or_default()
                .to_ascii_lowercase();
            match http.as_str() {
                // 外来文档给不出 token。空串表示「用 bearer，token 待填」：
                // 发送链路因此不会加 Authorization 头，并把它报成缺失变量。
                "bearer" => Some(AuthConfig {
                    kind: AuthType::Bearer,
                    bearer: Some(String::new()),
                    ..Default::default()
                }),
                "basic" => Some(AuthConfig {
                    kind: AuthType::Basic,
                    basic: Some(BasicAuth::default()),
                    ..Default::default()
                }),
                other => {
                    ctx.diagnostics.push(
                        Diagnostic::warn(&at, "http 认证方案不是 bearer/basic，无法导入")
                            .dropping(format!("http {other}")),
                    );
                    None
                }
            }
        }
        "apiKey" => {
            let key = text(scheme, "name").unwrap_or_default();
            let location = match text(scheme, "in").unwrap_or_default().as_str() {
                "header" => ApiKeyLocation::Header,
                "query" => ApiKeyLocation::Query,
                other => {
                    ctx.diagnostics.push(
                        Diagnostic::warn(
                            &at,
                            "ApiKeyLocation 只有 header 与 query，这个 apiKey 无法导入",
                        )
                        .dropping(format!("apiKey in: {other}")),
                    );
                    return None;
                }
            };
            Some(AuthConfig {
                kind: AuthType::Apikey,
                apikey: Some(ApiKeyAuth {
                    key,
                    value: String::new(),
                    in_location: location,
                }),
                ..Default::default()
            })
        }
        other => {
            ctx.diagnostics.push(
                Diagnostic::warn(&at, "OpenID Connect 与 oauth2 的配置结构对不上，无法导入")
                    .dropping(other.to_string()),
            );
            None
        }
    }
}

/// 文档级 `servers[0]` → host + basePath。
fn doc_servers(value: Option<&Value>, ctx: &mut Ctx) -> DocServers {
    let none = DocServers {
        hosts: Vec::new(),
        base_path: String::new(),
    };
    let Some(Value::Array(list)) = value else {
        if value.is_some() {
            ctx.diagnostics.push(Diagnostic::warn(
                "/servers",
                "servers 不是数组，已按没有服务器导入",
            ));
        }
        return none;
    };
    let Some(first) = list.first() else {
        return none;
    };
    let url = first.get("url").and_then(Value::as_str);
    let Some(url) = url else {
        ctx.diagnostics.push(Diagnostic::warn(
            "/servers/0",
            "servers[0] 没有 url，已按没有服务器导入",
        ));
        return none;
    };

    // 一个模块只能有一个服务，多服务器在这套模型里没有落点。
    for extra in list.iter().skip(1) {
        ctx.diagnostics.push(
            Diagnostic::warn("/servers", "多服务器无法导入，只取第一个").dropping(
                extra
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("（没有 url）")
                    .to_string(),
            ),
        );
    }

    let (host, base_path) = split_server_url(url);
    let hosts = if host.is_empty() {
        ctx.diagnostics.push(Diagnostic::warn(
            "/servers/0",
            "server url 里没有 host，前缀保留在路径上",
        ));
        Vec::new()
    } else {
        vec![IrServer {
            service: service_name(&host),
            url: host,
        }]
    };
    DocServers { hosts, base_path }
}

/// `https://api.example.com/v2` → (`https://api.example.com`, `/v2`)。
/// 没有 host（`/v2` 这样的相对写法）时 host 为空串，basePath 照留。
fn split_server_url(url: &str) -> (String, String) {
    let trimmed = url.trim();
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    // scheme 属于 URL 而不属于路径，故 host 要带着它。
    let scheme_end = without_query.find("://").map(|at| at + 3).unwrap_or(0);
    let rest = &without_query[scheme_end..];
    let end = rest.find('/').unwrap_or(rest.len());
    (
        without_query[..scheme_end + end].to_string(),
        rest[end..].trim_end_matches('/').to_string(),
    )
}

/// 服务名。端口与 scheme 不算名字的一部分；带变量的 host 回落成 `default`。
fn service_name(host: &str) -> String {
    let without_scheme = match host.find("://") {
        Some(at) => &host[at + 3..],
        None => host,
    };
    let bare = without_scheme.split(':').next().unwrap_or(without_scheme);
    if bare.is_empty() || bare.contains('{') || bare.contains('/') {
        "default".to_string()
    } else {
        bare.to_string()
    }
}

// `$ref` 展开

/// 一个待展开的槽：对象的键，或「数组里的下一个」。数组下标不记，因为展开失败的那
/// 几条会被丢掉，剩下的按原顺序压紧即可。
enum Slot {
    Key(String),
    Index,
}

/// 一条待展开的任务。`input` 自带一份副本：工作栈上跨迭代存活的值不能是借用，
/// 否则子任务就得指着一个每轮都被换掉的局部变量。
struct Task {
    input: Value,
    pointer: String,
    depth: usize,
    id: usize,
}

/// 工作栈上的一帧。
enum Frame {
    Task(Task),
    /// 子任务都回来了，按槽位拼出这个节点自己的值。
    Merge {
        id: usize,
        slots: Vec<(Slot, usize)>,
        base: Value,
        /// 本帧压进 [`Ctx::active`] 的 ref 指针，合并完弹回。
        opened: Vec<String>,
    },
}

/// 把一棵子树里的 `$ref` 全部展开掉。
///
/// 命中访问栈即成环，超深度即截断，两种情况都只截断**那一支**并出 `Warn`。
/// 显式工作栈而非递归：外来文档的嵌套深度不受本仓库约束。
fn deref(ctx: &mut Ctx, root: &Value, input: &Value, pointer: &str) -> Option<Value> {
    let mut results: Vec<Option<Value>> = Vec::new();
    let root_id = results.len();
    results.push(None);
    let mut stack = vec![Frame::Task(Task {
        input: input.clone(),
        pointer: pointer.to_string(),
        depth: 0,
        id: root_id,
    })];

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Task(task) => {
                // 展开失败时 `resolve` 没有把任何指针压进访问栈，故不必回退。
                let Some((node, opened)) =
                    resolve(ctx, root, &task.input, &task.pointer, task.depth)
                else {
                    continue;
                };
                match node {
                    Value::Object(map) if !map.is_empty() => {
                        let children: Vec<(String, Value)> = map.into_iter().collect();
                        let mut slots = Vec::with_capacity(children.len());
                        for (key, _) in &children {
                            slots.push((Slot::Key(key.clone()), results.len()));
                            results.push(None);
                        }
                        ctx.active.extend(opened.iter().cloned());
                        // Merge 先入栈：栈是后进先出，故它落在**所有**子任务之下，
                        // 也就是它们都回来之后才轮到它。
                        let ids: Vec<usize> = slots.iter().map(|(_, id)| *id).collect();
                        stack.push(Frame::Merge {
                            id: task.id,
                            slots,
                            base: Value::Object(Map::new()),
                            opened,
                        });
                        for (index, (key, child)) in children.into_iter().enumerate() {
                            stack.push(Frame::Task(Task {
                                input: child,
                                pointer: format!("{}/{}", task.pointer, escape(&key)),
                                depth: task.depth + 1,
                                id: ids[index],
                            }));
                        }
                    }
                    Value::Array(items) if !items.is_empty() => {
                        let mut slots = Vec::with_capacity(items.len());
                        for _ in 0..items.len() {
                            slots.push((Slot::Index, results.len()));
                            results.push(None);
                        }
                        ctx.active.extend(opened.iter().cloned());
                        let ids: Vec<usize> = slots.iter().map(|(_, id)| *id).collect();
                        stack.push(Frame::Merge {
                            id: task.id,
                            slots,
                            base: Value::Array(Vec::new()),
                            opened,
                        });
                        for (index, child) in items.into_iter().enumerate() {
                            stack.push(Frame::Task(Task {
                                input: child,
                                pointer: format!("{}/{index}", task.pointer),
                                depth: task.depth + 1,
                                id: ids[index],
                            }));
                        }
                    }
                    scalar => {
                        release(ctx, &opened);
                        results[task.id] = Some(scalar);
                    }
                }
            }
            Frame::Merge {
                id,
                slots,
                mut base,
                opened,
            } => {
                // 展开失败（成环 / 指不到）的槽位不填，等于丢掉那一条；数组因此变短。
                for (slot, child) in slots {
                    let Some(value) = results[child].take() else {
                        continue;
                    };
                    match (&mut base, slot) {
                        (Value::Object(map), Slot::Key(key)) => {
                            map.insert(key, value);
                        }
                        (Value::Array(items), Slot::Index) => items.push(value),
                        _ => {}
                    }
                }
                release(ctx, &opened);
                results[id] = Some(base);
            }
        }
    }
    results[root_id].take()
}

/// 沿 `$ref` 走查出真实节点，并把 `$ref` 旁边的兄弟字段合并上去（兄弟优先，3.1 语义）。
///
/// 返回值第二项是本次压进访问栈的指针，由调用方在子树展开完后弹回。
fn resolve(
    ctx: &mut Ctx,
    root: &Value,
    node: &Value,
    pointer: &str,
    depth: usize,
) -> Option<(Value, Vec<String>)> {
    let mut opened: Vec<String> = Vec::new();
    let mut current = node.clone();
    let mut hops = 0usize;

    loop {
        let Value::Object(map) = &current else {
            return Some((current, opened));
        };
        let Some(Value::String(reference)) = map.get("$ref") else {
            return Some((current, opened));
        };
        hops += 1;
        if depth + hops > MAX_REF_DEPTH {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 嵌套太深，已截断该分支")
                    .dropping(reference.clone()),
            );
            return None;
        }
        if opened.iter().any(|p| p == reference) || ctx.active.contains(reference) {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 成环，已停止展开该分支")
                    .dropping(reference.clone()),
            );
            return None;
        }
        if !reference.starts_with('#') {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "外部文件里的 $ref 无法解析").dropping(reference.clone()),
            );
            return None;
        }
        let Some(target) = pointer_lookup(root, reference) else {
            ctx.diagnostics.push(
                Diagnostic::warn(pointer, "$ref 指不到东西，该分支已丢弃")
                    .dropping(reference.clone()),
            );
            return None;
        };

        opened.push(reference.clone());
        // 兄弟字段覆盖被指向的内容（3.1 语义）。3.0 的工具链也这么写，故两条都收。
        let mut merged = target.clone();
        if let Some(target_map) = merged.as_object_mut() {
            for (key, value) in map {
                if key != "$ref" {
                    target_map.insert(key.clone(), value.clone());
                }
            }
        }
        current = merged;
    }
}

/// `#/components/schemas/Foo` 逐段走查。`~1` 是 `/`，`~0` 是 `~`。
fn pointer_lookup<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let rest = reference.strip_prefix("#/")?;
    let mut current = root;
    for raw in rest.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        current = current.get(&token)?;
    }
    Some(current)
}

/// 把本帧压进去的 ref 指针弹回访问栈。
fn release(ctx: &mut Ctx, opened: &[String]) {
    for _ in opened {
        ctx.active.pop();
    }
}

// 小工具

fn text(o: &Map<String, Value>, key: &str) -> Option<String> {
    o.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 标量存线格式；其它类型存它的 JSON 文本。
fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// media type 归一：`Application/JSON; charset=utf-8` 与 `application/json` 同一个。
fn normalize_media(raw: &str) -> String {
    raw.split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// JSON Pointer 转义：`/` 与 `~` 不能原样出现在指针里。
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

// 测试——$ref 展开、servers 拆分、多 media type 与诊断。

#[cfg(test)]
#[path = "openapi_test.rs"]
mod tests;
