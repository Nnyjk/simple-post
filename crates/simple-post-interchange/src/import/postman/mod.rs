//! Postman Collection v2.1 → IR。
//!
//! 由 [`super::parse_text`] 分派进来；本模块只管 [`serde_json::Value`]。
//!
//! 三条纪律贯穿全文件：
//!
//! - **表达不了的构造必须出声。** 每一处降级映射都写一条 [`Diagnostic`]：静默丢一个
//!   字段与没写这个字段的解析器，对用户是同一件事。
//! - **路径是 path-only。** host 归 [`IrModule::base_url`] 与
//!   [`crate::ir::IrEndpoint::base_url_override`]，query 归
//!   [`crate::ir::IrEndpoint::params`]；拆不出 host 时整串留在 path 里——留一个错的路径，
//!   好过静默丢掉整条 URL。
//! - **嵌套深度无上限。** folder 树用显式工作栈遍历，故一棵畸形外来文件只会拿到一堆
//!   诊断，不会把预览进程带走。

mod auth;
mod body;
mod request;
mod script;
mod template;
mod tree;

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};
use simple_post_core::domain::{uid, Environment, KeyValue};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrBaseUrl, IrModule, IrScripts};

use self::request::{build_endpoint, rebuild_raw};
use self::script::{parse_scripts, Place};
use self::tree::{classify, walk_children, Class};
use super::ParsedExchange;

/// 解析期的累积状态。
#[derive(Default)]
pub(super) struct Ctx {
    pub(super) diagnostics: Vec<Diagnostic>,
    /// 已报过的坏 `{{...}}` token（整串）。一份文件里同一个 token 只报一次：
    /// 同一个变量名出现十次不代表十个问题。
    pub(super) seen_bad_tokens: HashSet<String>,
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
    // （落库侧的 `CommitTarget::Module`），故「Postman 顶层 folder → 模块下的
    // 集合」不需要额外映射。
    // 顶层散落的请求（不是 folder 的 item）归到**同一个**模块里，名字取 `info.name`。
    // 每个顶层 item 各成一个模块的话，一份「请求平铺在根上」的集合会变成 N 个各含 1 个
    // 接口的集合 —— 与 OpenAPI 侧「无 tag 归到一个分组」是同一条路，见
    // `import/openapi` 的 `group_of`。folder 仍各自成一个模块：Postman 的 folder 树
    // 是有意义的分组，不能合并成一层。
    let mut loose_at: Option<usize> = None;

    if let Some(top) = top {
        for (i, item) in top.iter().enumerate() {
            let ptr = format!("/item/{i}");
            let Some(obj) = item.as_object() else {
                ctx.diagnostics
                    .push(Diagnostic::error(&ptr, "这一项不是 JSON 对象，已跳过"));
                continue;
            };

            match classify(obj, &ptr, &mut ctx) {
                Class::Request(req) => {
                    let mut endpoints = Vec::new();
                    // 脚本挂在**条目**上，不在 `request` 里。
                    let scripts = parse_scripts(obj, &ptr, Place::Node, &mut ctx);
                    if let Some(ep) = build_endpoint(req, item_name(obj), &ptr, scripts, &mut ctx) {
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
                                base_url: None,
                                endpoints: Vec::new(),
                                collections: Vec::new(),
                                scripts: IrScripts::default(),
                            });
                            modules.len() - 1
                        }
                    };
                    loose_at = Some(at);
                    modules[at].endpoints.extend(endpoints);
                }
                Class::Folder(items) => {
                    // 顶层 folder 会落成**模块**（挂进已有模块时降级成集合）。模块不能存
                    // 脚本，故脚本先读出来带着，落库侧再按落点决定写不写。
                    let scripts = parse_scripts(obj, &ptr, Place::Node, &mut ctx);
                    let c = walk_children(items, &ptr, &mut ctx);
                    // 空容器不进树：预览里挂一个什么都没有的模块，只会让人以为少导了东西
                    // （落库侧同样跳过空容器，见 `ir::subtree_keys`）。
                    if c.endpoints.is_empty() && c.collections.is_empty() {
                        continue;
                    }
                    modules.push(IrModule {
                        key: ptr,
                        name: item_name(obj),
                        description: description_text(obj.get("description")),
                        base_url: None,
                        endpoints: c.endpoints,
                        collections: c.collections,
                        scripts,
                    });
                }
                Class::Malformed => continue,
            }
        }
    }

    // 一个模块一格：格子里的地址取这个模块子树里第一个端点的地址。模块里的第二个
    // 地址没有地方放，那些端点各自带着自己的地址（`IrEndpoint::base_url_override`）。
    assign_module_base_urls(&mut modules);

    let environments = parse_variables(root, &project_name);

    // 整份集合根上的 `event[]`（整个集合的前置 / 测试脚本）**无处可写**。
    //
    // 本仓库的脚本住在端点与集合上，而「整份集合」不是一个节点：顶层条目各自成一个
    // 模块，挂进已有模块时那些模块降级成集合。故根上的脚本挂不到任何一个上——
    // 逐条出诊断说明，不静默丢。
    parse_scripts(root, "", Place::CollectionRoot, &mut ctx);

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
// environment / 小工具
// -------------------------------------------------------------------------

/// 给每个模块定下它那一格。
///
/// 一个模块一格，故格子里的地址取这个模块子树里第一个端点的地址。模块里的第二个
/// 地址没有地方放：那些端点各自带着自己的地址（`IrEndpoint::base_url_override`），
/// 不套用模块这一格。Postman 的 `host` 逐条给，故这里从端点那里回收。
fn assign_module_base_urls(modules: &mut [IrModule]) {
    for m in modules.iter_mut() {
        let first = {
            let endpoints = crate::ir::endpoints_of(m);
            endpoints
                .iter()
                .find_map(|e| e.base_url_override.clone())
                .filter(|url| !url.is_empty())
        };
        m.base_url = first.map(|url| IrBaseUrl { url, proxy: None });
    }
}

/// 文档自带的 `variable[]` → 一个环境。
///
/// 它是**收集变量**，不是地址：导成模块那一格会凭空造出一台不存在的机器。
/// `is_active` 恒为 false：导入不抢激活位。
fn parse_variables(root: &Map<String, Value>, name: &str) -> Vec<Environment> {
    let Some(arr) = root.get("variable").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut variables = HashMap::new();
    let mut disabled_variables: Vec<String> = Vec::new();
    for entry in arr {
        let Some(o) = entry.as_object() else { continue };
        let Some(k) = field_text(o, "key").filter(|k| !k.is_empty()) else {
            continue;
        };
        // `disabled: true` 是作者刻意关掉这个变量：不参与替换。丢掉它的话，导入后这个变量
        // 变成启用的，开始替换本该被遮住的值。
        if o.get("disabled").and_then(Value::as_bool) == Some(true) {
            disabled_variables.push(k.clone());
        }
        variables.insert(k, field_text(o, "value").unwrap_or_default());
    }
    if variables.is_empty() {
        return Vec::new();
    }
    disabled_variables.sort();
    vec![Environment {
        id: uid(),
        project_id: String::new(),
        name: name.to_string(),
        module_base_urls: HashMap::new(),
        variables,
        disabled_variables,
        is_active: false,
    }]
}

/// 条目名。名字是树里唯一能看见的东西，空着等于让用户对着一棵无名树。
pub(super) fn item_name(obj: &Map<String, Value>) -> String {
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
pub(super) fn kv_list(value: Option<&Value>, ptr: &str, ctx: &mut Ctx) -> Vec<KeyValue> {
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
pub(super) fn field_text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub(super) fn flag(obj: &Map<String, Value>, key: &str) -> bool {
    matches!(obj.get(key), Some(Value::Bool(true)))
}

/// 描述可以是裸字符串，也可以是 `{content, type}`。
pub(super) fn description_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => field_text(o, "content").filter(|s| !s.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod support;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_tests;

#[cfg(test)]
#[path = "tree_test.rs"]
mod tree_tests;

#[cfg(test)]
#[path = "request_test.rs"]
mod request_tests;

#[cfg(test)]
#[path = "body_test.rs"]
mod body_tests;

#[cfg(test)]
#[path = "auth_test.rs"]
mod auth_tests;

#[cfg(test)]
#[path = "template_test.rs"]
mod template_tests;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_tests;
