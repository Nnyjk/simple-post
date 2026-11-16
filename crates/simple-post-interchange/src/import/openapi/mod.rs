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
//! 分组只有一个依据：**operation 的 `tags[0]`**。没打 tag 的归到一个以 `info.title`
//! 命名的分组里——文档没给的信息不猜，而「每个 operation 一个分组」会把一份 87 个接口
//! 的文档变成 87 个同名集合。落进已有模块时 IR 顶层模块一律降级成集合，故「tag → 目标
//! 模块下的集合」不需要额外映射，与 Postman 导入器同一条路。
//!
//! 结构兼容的 3.1 文档也收：联合类型数组（`["string","null"]`）按可选处理，`$ref`
//! 旁边的兄弟字段按覆盖处理。不做 JSON Schema 2020-12 求值，也不碰
//! `webhooks` / `jsonSchemaDialect` / `components.pathItems`。

mod auth;
mod params;
mod refs;
mod servers;

use std::collections::HashMap;

use serde_json::{Map, Value};
use simple_post_core::domain::{parse_http_method, HttpMethod, ResponseDefinition};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrBaseUrl, IrEndpoint, IrModule, IrScripts};
use crate::spec::{escape_pointer_token, normalize_media, PATH_METHODS};

use self::auth::auth_of;
use self::params::{parameters, request_body};
use self::servers::doc_servers;
use super::ParsedExchange;

/// `requestBody.content` 的偏好序：先到先得。`RequestBody` 只有一个 `content`，
/// 装不下第二个。
pub(super) const CONTENT_PREFERENCE: [&str; 3] = [
    "application/json",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
];

/// `$ref` 展开的深度上限。没有它，一层套一层的 `$ref` 就能把导入器拖爆。
pub(super) const MAX_REF_DEPTH: usize = 24;

/// 既没有 tag 也没有 `info.title` 时的名字。
const DEFAULT_NAME: &str = "默认";

/// 没有 tag 的 operation 归处的那个分组。
///
/// 它指「整份 `paths`」这一个位置，而不是某个 operation 的指针：文档根本没给分组信息，
/// 而分组是要摆到用户面前的一层树，凭空按路径前缀切一刀就是**猜**。
const UNTAGGED_KEY: &str = "/paths";

/// 解析期的累积状态。
#[derive(Default)]
pub(super) struct Ctx {
    pub(super) diagnostics: Vec<Diagnostic>,
    /// 当前访问栈上的 `$ref` 指针。命中即成环。
    pub(super) active: Vec<String>,
}

/// 逐个 operation 都要用到的东西，打包一次传下去。
pub(super) struct DocView<'a> {
    pub(super) root: &'a Value,
    pub(super) title: Option<&'a str>,
    pub(super) servers: &'a DocServers,
}

/// `servers[0].url` 拆完之后的样子。
pub(super) struct DocServers {
    /// 文档级的地址。一个模块一格，故这份文档只给得出一个地址。
    pub(super) host: Option<String>,
    /// 留在 path 上的前缀。
    pub(super) base_path: String,
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

    // 分组按首次出现排序，故顺序由 `paths` 的键序与固定动词序决定，是确定的。
    let mut modules: Vec<IrModule> = Vec::new();
    let mut group_at: HashMap<String, usize> = HashMap::new();
    for (raw_path, item) in paths {
        let pointer = format!("/paths/{}", escape_pointer_token(raw_path));
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
            let at = group_of(
                &mut modules,
                &mut group_at,
                &view,
                operation,
                &op_pointer,
                &mut ctx,
            );
            modules[at].endpoints.push(endpoint_of(
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

/// 这个 operation 落进哪个分组，分组不存在就建。返回它在 `modules` 里的下标。
///
/// **只认 `tags[0]`。** 同一个 operation 复制到第二个 tag 下会造出两条指纹相同的接口，
/// 落库时第二条被当成重复跳过——预览里看起来像「自己导了两份」。故只归第一处，并把
/// 多余的 tag 点名丢掉。
fn group_of(
    modules: &mut Vec<IrModule>,
    group_at: &mut HashMap<String, usize>,
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
    ctx: &mut Ctx,
) -> usize {
    let tags = string_list(operation.get("tags"));
    for extra in tags.iter().skip(1) {
        ctx.diagnostics.push(
            Diagnostic::warn(
                format!("{pointer}/tags"),
                "一个接口只归第一个 tag，其余的 tag 不生成分组",
            )
            .dropping(extra.clone()),
        );
    }

    let (key, name) = match tags.first() {
        Some(tag) => (format!("/tags/{}", escape_pointer_token(tag)), tag.clone()),
        None => (
            UNTAGGED_KEY.to_string(),
            view.title.unwrap_or(DEFAULT_NAME).to_string(),
        ),
    };
    if let Some(&at) = group_at.get(&key) {
        return at;
    }
    let at = modules.len();
    modules.push(IrModule {
        key: key.clone(),
        name,
        description: None,
        base_url: view
            .servers
            .host
            .clone()
            .map(|url| IrBaseUrl { url, rewrite: None }),
        endpoints: Vec::new(),
        collections: Vec::new(),
        scripts: IrScripts::default(),
    });
    group_at.insert(key, at);
    at
}

/// 一个 operation → 一个 endpoint。
fn endpoint_of(
    view: &DocView,
    raw_path: &str,
    verb: &str,
    operation: &Map<String, Value>,
    item: &Map<String, Value>,
    ctx: &mut Ctx,
) -> IrEndpoint {
    let key = format!("/paths/{}/{}", escape_pointer_token(raw_path), verb);
    let path = format!("{}{raw_path}", view.servers.base_path);
    let tags = string_list(operation.get("tags"));

    // 固定动词键集里没有一个解析不了，故这里不需要回退。
    let method = parse_http_method(verb).unwrap_or(HttpMethod::Get);
    let name = text(operation, "summary")
        .or_else(|| text(operation, "operationId"))
        .unwrap_or_else(|| format!("{} {path}", verb.to_ascii_uppercase()));

    let (params, path_params, headers) = parameters(ctx, view, item, operation, &key);
    let body = request_body(ctx, view, operation, &key);
    let auth = auth_of(ctx, view, operation, &key);
    let responses = responses_of(view, operation, &key, ctx);

    IrEndpoint {
        key,
        name,
        description: text(operation, "description"),
        method,
        path,
        // 文档级的地址归模块那一格，故端点不带自己的地址。
        base_url_override: None,
        params,
        path_params,
        headers,
        body,
        auth,
        responses,
        docs: None,
        notes: None,
        tags,
        scripts: IrScripts::default(),
    }
}

// 小工具

/// 一个 operation 的 `responses` → 响应定义，每个 status 键一条。
///
/// `$ref` 先展开：`components/responses` 里那一份就是一条完整的响应。表达不了的构造逐项
/// 回诊断点名丢掉——`headers`、`links`、一个响应里的多个 media type、`examples` 里的多个
/// 命名示例，本仓库的响应定义各只有一格，装不下它们。
///
/// `status` 键不认识（既不是三位状态码也不是 `2XX` / `default`）时整条丢弃：存进去之后既
/// 导不回 OpenAPI，界面上也无从解释它是哪一种。
fn responses_of(
    view: &DocView,
    operation: &Map<String, Value>,
    pointer: &str,
    ctx: &mut Ctx,
) -> Vec<ResponseDefinition> {
    let base = format!("{pointer}/responses");
    let Some(declared) = operation.get("responses") else {
        return Vec::new();
    };
    let Some(responses) = declared.as_object() else {
        ctx.diagnostics.push(Diagnostic::warn(
            &base,
            "responses 不是对象，这一批响应定义全部丢弃",
        ));
        return Vec::new();
    };
    if responses.is_empty() {
        ctx.diagnostics.push(Diagnostic::warn(
            &base,
            "responses 是空对象，规范要求至少有一个成员，故没有可导入的定义",
        ));
        return Vec::new();
    }

    let mut out = Vec::with_capacity(responses.len());
    for (status, raw) in responses {
        let slot = format!("{base}/{}", escape_pointer_token(status));
        let Some(node) = refs::deref(ctx, view.root, raw, &slot) else {
            continue;
        };
        let Some(response) = node.as_object() else {
            ctx.diagnostics
                .push(Diagnostic::warn(&slot, "响应定义不是对象，这一条被丢弃"));
            continue;
        };
        if !simple_post_core::domain::is_response_status(status) {
            ctx.diagnostics.push(
                Diagnostic::warn(
                    &slot,
                    "status 键必须是三位状态码、`2XX` 这样的范围或 `default`，本仓库的响应定义只认这三种",
                )
                .dropping(status.clone()),
            );
            continue;
        }
        for (key, what) in [("headers", "响应头"), ("links", "响应链接")] {
            if response.get(key).is_some_and(|v| !v.is_null()) {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        format!("{slot}/{key}"),
                        format!("OpenAPI 的{what}在本仓库的响应定义里没有对应字段，已丢弃"),
                    )
                    .dropping(serde_json::to_string(&response[key]).unwrap_or_default()),
                );
            }
        }

        let description = text(response, "description").unwrap_or_default();
        if description.is_empty() {
            ctx.diagnostics.push(Diagnostic::info(
                format!("{slot}/description"),
                "响应没有 description。规范要求必填，故导回去会是一句空串，界面上会提示补一句",
            ));
        }
        let (content_type, schema, example) = response_content(response, &slot, ctx);
        out.push(ResponseDefinition {
            // id 由落库那一侧铸：导入只负责内容，与 endpoint 的 key 同一条约定。
            id: String::new(),
            status: status.clone(),
            description,
            content_type,
            schema,
            example,
        });
    }
    out
}

/// 一条响应的 `content` → `(contentType, schema, example)`。
///
/// media type 的选择与 `requestBody` 同一条偏好序：一格里只放得下一个，其余的丢并点名。
fn response_content(
    response: &Map<String, Value>,
    slot: &str,
    ctx: &mut Ctx,
) -> (String, Option<Value>, Option<String>) {
    let Some(content) = response.get("content").and_then(Value::as_object) else {
        return (String::new(), None, None);
    };
    let Some((raw_key, media)) = pick_media(content, ctx, slot) else {
        return (String::new(), None, None);
    };
    let Some(media_object) = content.get(&raw_key).and_then(Value::as_object) else {
        return (media, None, None);
    };

    let mut schema = None;
    if let Some(declared) = media_object.get("schema") {
        if declared.is_object() {
            schema = Some(declared.clone());
        } else {
            ctx.diagnostics.push(Diagnostic::warn(
                format!("{slot}/content/{raw_key}/schema"),
                "schema 不是对象（一份 JSON Schema），已丢弃",
            ));
        }
    }

    let mut example = None;
    if let Some(declared) = media_object.get("example") {
        example = Some(example_text(declared));
    } else if let Some(Value::Object(named)) = media_object.get("examples") {
        match named.len() {
            0 => {}
            1 => {
                example = named
                    .values()
                    .next()
                    .and_then(|entry| entry.get("value"))
                    .map(example_text);
            }
            _ => {
                let names: Vec<&str> = named.keys().map(String::as_str).collect();
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        format!("{slot}/content/{raw_key}/examples"),
                        "一个响应只带得下一份示例，这里有多份，全部丢弃",
                    )
                    .dropping(names.join(", ")),
                );
            }
        }
    }
    (media, schema, example)
}

/// 留哪一个 media type：先按 `CONTENT_PREFERENCE`，都没有就取字典序第一个。
///
/// 返回值第一项是文档里原来的键（带参数的那个），第二项是归一后的值——存进响应定义的是
/// 归一后的那一个，`charset` 之类的参数属于报文而不属于「这个响应是什么类型」。
fn pick_media(content: &Map<String, Value>, ctx: &mut Ctx, slot: &str) -> Option<(String, String)> {
    let mut chosen = CONTENT_PREFERENCE.iter().find_map(|want| {
        content
            .keys()
            .find(|k| normalize_media(k) == *want)
            .cloned()
    });
    if chosen.is_none() {
        chosen = content.keys().next().cloned();
    }
    let raw_key = chosen?;
    let media = normalize_media(&raw_key);
    if !simple_post_core::domain::is_media_type(&media) {
        ctx.diagnostics.push(
            Diagnostic::warn(
                format!("{slot}/content"),
                "content 的键不是合法的 media type（要写成 `type/subtype`），这一条响应的 content 被丢弃",
            )
            .dropping(raw_key.clone()),
        );
        return None;
    }
    for key in content.keys() {
        if normalize_media(key) != media {
            ctx.diagnostics.push(
                Diagnostic::warn(
                    format!("{slot}/content"),
                    "一个响应只带得下一份 content，其余 media type 已丢弃",
                )
                .dropping(key.clone()),
            );
        }
    }
    Some((raw_key, media))
}

pub(super) fn text(o: &Map<String, Value>, key: &str) -> Option<String> {
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
pub(super) fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 示例原文按 JSON 写法存，**字符串保留引号**。
///
/// 导出侧把这段文本 `serde_json::from_str` 回成值写进 `example`，故存的必须是带类型的 JSON
/// 文本：存成裸的 `123` / `true` / `null` 会在下一次导出时变成数字、布尔与 null，把一份
/// 字符串示例静默改型。对象与数组两种写法相同，故这里不需要分支。
fn example_text(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

// 测试——$ref 展开、servers 拆分、多 media type 与诊断。

#[cfg(test)]
mod support;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_tests;

#[cfg(test)]
#[path = "refs_test.rs"]
mod refs_tests;

#[cfg(test)]
#[path = "servers_test.rs"]
mod servers_tests;

#[cfg(test)]
#[path = "params_test.rs"]
mod params_tests;

#[cfg(test)]
#[path = "auth_test.rs"]
mod auth_tests;
