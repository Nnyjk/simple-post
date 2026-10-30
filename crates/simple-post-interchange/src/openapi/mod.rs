//! IR → OpenAPI 3.0 文档。
//!
//! 这条边是**投影**：IR 里没有的构造文档里就不写。IR 里有而 OpenAPI 表达不了的，
//! 必须回诊断（见 [`Document::diagnostics`]）——「导出成功」四个字会让用户看不见
//! 自己少了什么。
//!
//! 三个约定是决策，不是实现细节：
//!
//! - **`responses` 合成默认，不猜。** `StoredResponse` 存的是实际响应，里面没有响应
//!   schema；请求体那份定义（`RequestBody.schema`）说的是**请求**，不能拿来当响应形状。
//!   故每个 operation 输出一个 `200: OK`。
//! - **`servers` 先看端点自己的地址，再逐模块取，不回退。** 端点配了自己的地址就写那个
//!   地址；端点没有自己的地址时取模块那一格，该模块没配就是没有 URL：不借用别的模块的。
//! - **`tags` 就是集合名。** tag 是 OpenAPI 里唯一的分组机制，不写它集合这层就在产物里
//!   消失，重新导入时全部塌成一个分组。tag 是扁平字符串，故只落**最外层**集合——
//!   见 [`document_tags`]。

mod builder;

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};
use simple_post_core::domain::BodyMode;
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrEndpoint};

use self::builder::{Builder, Pending};

/// 产出的文档格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Json,
    Yaml,
}

impl DocFormat {
    /// 从线上名认格式。认不出就报 `Validation`——**不静默回退成 JSON**：
    /// 用户要的是 YAML，而拿到一份 JSON 时没有任何线索告诉他拿错了。
    pub fn parse(s: &str) -> Result<DocFormat, AppError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(DocFormat::Json),
            "yaml" => Ok(DocFormat::Yaml),
            other => Err(AppError::Validation(format!(
                "不认识的文档格式 {other:?}，只有 json 与 yaml"
            ))),
        }
    }

    pub fn media_type(&self) -> &'static str {
        match self {
            DocFormat::Json => "application/json",
            DocFormat::Yaml => "application/yaml",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            DocFormat::Json => ".json",
            DocFormat::Yaml => ".yaml",
        }
    }

    fn serialize(&self, doc: &Value) -> Result<String, AppError> {
        match self {
            DocFormat::Json => serde_json::to_string_pretty(doc).map_err(AppError::Serde),
            DocFormat::Yaml => serde_norway::to_string(doc).map_err(|e| {
                AppError::Serde(serde_json::Error::io(std::io::Error::other(e.to_string())))
            }),
        }
    }
}

/// 一份产好的文档，以及落它期间产生的诊断。
#[derive(Debug, Clone)]
pub struct Document {
    pub content: String,
    /// 跳过 / 降级映射的那些构造。它们不构成错误——文档仍然是合法的。
    pub diagnostics: Vec<Diagnostic>,
}

/// `IrEndpoint.body.mode` → OpenAPI media type。`BodyMode::None` 没有 media type，
/// 故返回 `None` 且不写 `requestBody`。
fn media_type_of(mode: &BodyMode) -> Option<&'static str> {
    match mode {
        BodyMode::None => None,
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Multipart => Some("multipart/form-data"),
        BodyMode::Xml => Some("application/xml"),
        BodyMode::Raw => Some("application/octet-stream"),
        BodyMode::Binary => Some("application/octet-stream"),
    }
}

/// `动词 + 路径` → 一个合法的 `operationId`。
///
/// 路径分隔符换成**双**下划线，其余非法字符换成单下划线：`operationId` 在多数代码
/// 生成器里会被直接变成方法名，而 `/` 与 `{}` 都不是标识符字符。不做大小写变换——
/// 那会抹掉用户路径里的区分信息。
///
/// 分隔符要与其余字符区分开，否则 `/api/user-info` 与 `/api/user/info` 撞名
/// （现在分别是 `get___api__user_info` 与 `get___api__user__info`）。
///
/// 映射仍不是**单射**（`/a-b` 与 `/a_b` 撞名），故另有一道编号回退（见
/// [`Builder::claim_operation_id`]）：规范要的是「文档内唯一」，不是「映射可逆」。
fn operation_id(verb: &str, path: &str) -> String {
    let mut out = String::with_capacity(verb.len() + path.len() + 1);
    out.push_str(verb);
    out.push('_');
    for c in path.chars() {
        match c {
            '/' => out.push_str("__"),
            c if c.is_ascii_alphanumeric() || c == '_' => out.push(c),
            _ => out.push('_'),
        }
    }
    out
}

/// OpenAPI 路径模板写 `{name}`，Postman 模板写 `{{name}}`。两者讲的是同一件事
/// ——「这一段替换成变量」，故直接换写，不是猜。
///
/// **不做换写会怎样**：`simple_post_core::path_params::placeholders` 取第一个 `{` 到
/// 其后第一个 `}` 之间，于是 `/users/{{userId}}` 得出 `{userId`（多带一个左括号），产出
/// 一份「参数名对应不上任何模板变量」的文档；而 `paths` 键里留着双花括号，这份文档过不了
/// 它自己的 `openapiv3` / Redocly 校验。
///
/// 配不上 `}}` 的原样留着，交给 [`path_key_is_expressible`] 去拒绝。
fn normalize_path_template(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(at) = rest.find("{{") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let Some(len) = tail[2..].find("}}") else {
            out.push_str(tail);
            return out;
        };
        out.push('{');
        out.push_str(&tail[2..2 + len]);
        out.push('}');
        rest = &tail[2 + len + 2..];
    }
    out.push_str(rest);
    out
}

/// 换写之后，这个 path 能不能当 OpenAPI 3.0 的 `paths` 键。
///
/// 规范要求键以 `/` 开头。Postman 的 `{{baseUrl}}/users` 是「整个 host 是一个变量」，
/// 导入时本来就因为拆不出稳定服务名而把整串留在 path 里（并出了一条诊断），换写之后
/// 它仍不以 `/` 开头——这份 path 里没有可导出的地址，本仓库也没有能表达它的形状。
/// 与其产出一份不合法的文档，不如跳过这条操作并说明原因。
fn path_key_is_expressible(path: &str) -> bool {
    path.starts_with('/') && !path.contains("{{")
}

/// 这个接口的 `parameters` 数组。`path` 是**换写后**的那一条（见
/// [`normalize_path_template`]），不是端点行里的原样。
///
/// **路径模板里的每个 `{占位符}` 都必须声明成 `in: path`**，否则 OpenAPI 工具读不出
/// 路径参数（Redocly 的 `path-parameters-defined` 会报错，代码生成器会漏掉那个参数）。
/// 三件事要一起做：
///
/// 1. 端点自己的**路径参数**（`IrEndpoint::path_params`）写成 `in: path`。
/// 2. 端点的**查询参数**（`IrEndpoint::params`）一律写成 `in: query` —— 两份数据
///    各有各的位置，不靠「参数名是否与占位符相同」来猜一个参数进哪儿。
/// 3. 占位符没有对应路径参数行的，**照样声明**。用户删了那一行但 URL 里占位符还在；
///    那时缺声明的是这份文档，不是用户的配置。
///
/// **请求头也在这里**，落成 `in: header` 的参数条目。OpenAPI 没有独立的「请求头」
/// 段落，头就是参数的一种位置；导入侧正是按 `in: header` 把它映回 `headers`
/// （见 `import/openapi/params.rs`），故导出侧写 `in: header`，「导出再导入」这条路
/// 才接得上——否则头在往返里整个消失。
///
/// **不写头的值。** 值里常有凭据与 `{{变量}}`，一份面向人的契约文档不该带它们；
/// query 参数同样只写名字（见上）。头**在不在**这件事由条目本身表达了。
fn parameters(
    ep: &IrEndpoint,
    path: &str,
    pointer: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Value> {
    let placeholders = simple_post_core::path_params::placeholders(path);
    let mut out: Vec<Value> = Vec::new();

    for p in ep.params.iter().filter(|p| p.enabled) {
        // 导出器不猜值的实际形态：启用即视为必填，schema 一律按 string 写。
        out.push(json!({
            "name": p.key,
            "in": "query",
            "required": true,
            "schema": { "type": "string" },
        }));
    }

    // 同一个路径参数名只能写一条：OpenAPI 要求 `(name, in)` 唯一，重复的条目会让
    // 文档不合规。导出器留下第一条，被丢的那条要**说出来**——不说是静默丢弃。
    let mut written_paths: BTreeSet<&str> = BTreeSet::new();
    for p in ep.path_params.iter().filter(|p| p.enabled) {
        // 导出器只写路径模板里真的存在的占位符。OpenAPI 规定 `in: path` 的参数名必须对应
        // 路径字段里的一个模板表达式，故写一个路径里没有的参数会让文档不合规，
        // Swagger Codegen 一类工具读它会报错或丢掉那个参数。
        //
        // 路径里没人认领的占位符由下面那段补声明，故导出器不写「模板里没有」的条目：
        // 那是用户改过占位符名字之后留下的旧条目，它不进请求。
        if !placeholders.contains(&p.key) {
            diagnostics.push(
                Diagnostic::info(
                    pointer,
                    format!(
                        "路径参数 {} 在当前路径里没有对应的占位符，没有写进文档",
                        p.key
                    ),
                )
                .dropping(format!("路径参数 {}", p.key)),
            );
            continue;
        }
        if !written_paths.insert(p.key.as_str()) {
            diagnostics.push(
                Diagnostic::info(
                    pointer,
                    "同一个路径参数出现了多次，OpenAPI 的参数名必须唯一，只写了第一条",
                )
                .dropping(format!("路径参数 {}", p.key)),
            );
            continue;
        }
        out.push(json!({
            "name": p.key,
            "in": "path",
            "required": true,
            "schema": { "type": "string" },
        }));
    }

    for name in &placeholders {
        if ep.path_params.iter().any(|p| p.enabled && &p.key == name) {
            continue;
        }
        out.push(json!({
            "name": name,
            "in": "path",
            "required": true,
            "schema": { "type": "string" },
        }));
    }

    // 同一个头名只能写一条：OpenAPI 要求 `(name, in)` 唯一，重复的条目会让文档不合规。
    // 导出器留下第一条，被丢的那条要**说出来**——不说是静默丢弃。
    let mut written: BTreeSet<&str> = BTreeSet::new();
    for h in ep.headers.iter().filter(|h| h.enabled) {
        let name = h.key.trim();
        if name.is_empty() {
            diagnostics.push(
                Diagnostic::warn(pointer, "这个请求头没有名字，无法写进文档")
                    .dropping("无名请求头"),
            );
            continue;
        }
        if !written.insert(name) {
            diagnostics.push(
                Diagnostic::info(
                    pointer,
                    "同一个请求头出现了多次，OpenAPI 的参数名必须唯一，只写了第一条",
                )
                .dropping(format!("请求头 {name}")),
            );
            continue;
        }
        out.push(json!({
            "name": name,
            "in": "header",
            "required": true,
            "schema": { "type": "string" },
        }));
    }

    out
}

/// 一个 operation 在文档里该带的 tag。
///
/// **端点自己的 tags 优先**：那是用户在请求上填的分类，与树结构无关；自己填了就
/// 不动它。空的时候用最外层集合的名字补上——tag 是 OpenAPI 里唯一的分组机制，不补
/// 的话集合这一层在产物里彻底消失。
///
/// **只落最外层**，因为 tag 是扁平字符串，集合的嵌套没有对应构造。取最内层的话，两个
/// 不同的顶层集合下各有一个同名子集合时会被写成同一个 tag，重新导入就并成一处。
fn document_tags(ep: &IrEndpoint, group: Option<&str>) -> Vec<String> {
    if !ep.tags.is_empty() {
        return ep.tags.clone();
    }
    group
        .filter(|g| !g.trim().is_empty())
        .map(|g| vec![g.to_string()])
        .unwrap_or_default()
}

/// OpenAPI 的 component 键只允许 `[A-Za-z0-9._-]`。非法字符换成 `_`。
fn component_key(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 一份 JSON 的规范文本：对象的键按字典序排好，故键序不同的两份内容得到同一段文本。
///
/// **不能靠 `serde_json` 的默认行为**：`Value::Object` 的底层容器保不保序，取决于依赖
/// 图里有没有人打开 `serde_json` 的 `preserve_order` 特性——那是特性统一的结果，本
/// crate 管不到。键序一变，「同一份定义」的判断就变了，故这里自己排一遍序。
fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// 规范文本的递归写法。对象按键序写，数组按原序写（数组的顺序是有意义的）。
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                if let Some(child) = map.get(*key) {
                    write_canonical(child, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// 规范文本的短标识：FNV-1a 128 位，取前 8 个十六进制字符。
///
/// 与 `fingerprint.rs` 用同一套算法与同样的两条理由：不依赖 `DefaultHasher`（它明确
/// 不保证跨版本稳定），也不为一个短标识拖进哈希依赖。
fn content_hash(canonical: &str) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    let mut hash = OFFSET;
    for b in canonical.as_bytes() {
        hash ^= *b as u128;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")[..8].to_string()
}

/// 把一份 [`Exchange`] 落成 OpenAPI 3.0 文档。
pub fn to_document(ex: &Exchange, format: DocFormat) -> Result<Document, AppError> {
    let mut b = Builder::default();

    // 显式工作栈而非递归：IR 的 collection 嵌套深度无上限。
    let mut stack: Vec<Pending> = ex.modules.iter().rev().map(Pending::module).collect();
    while let Some(p) = stack.pop() {
        for ep in p.endpoints {
            b.operation(ep, p.base_url, p.group);
        }
        // 集合与模块上的脚本同样表达不了。集合没有文档位置（它落成 tag），故指针
        // 指回它在树里的身份——导出侧的 `key` 就是 `nodes.id`。
        for c in p.collections {
            b.report_scripts(&c.scripts, &format!("/nodes/{}", c.key), "集合");
        }
        stack.extend(
            p.collections
                .iter()
                .rev()
                .map(|c| Pending::collection(c, p.base_url, p.group)),
        );
    }
    for m in &ex.modules {
        b.report_scripts(&m.scripts, &format!("/nodes/{}", m.key), "模块");
    }

    let mut doc = Map::new();
    doc.insert("openapi".into(), json!("3.0.3"));
    let mut info = Map::new();
    info.insert("title".into(), json!(ex.project_name));
    info.insert("version".into(), json!("0.1.0"));
    if let Some(d) = &ex.project_description {
        info.insert("description".into(), json!(d));
    }
    doc.insert("info".into(), Value::Object(info));
    doc.insert("paths".into(), Value::Object(b.paths));

    let mut components = Map::new();
    if !b.schemes.is_empty() {
        components.insert("securitySchemes".into(), Value::Object(b.schemes));
    }
    if !b.schemas.is_empty() {
        components.insert("schemas".into(), Value::Object(b.schemas));
    }
    if !components.is_empty() {
        doc.insert("components".into(), Value::Object(components));
    }
    if !b.tags.is_empty() {
        doc.insert(
            "tags".into(),
            Value::Array(b.tags.iter().map(|t| json!({ "name": t })).collect()),
        );
    }

    Ok(Document {
        content: format.serialize(&Value::Object(doc))?,
        diagnostics: b.diagnostics,
    })
}

// 测试——文档形状、动词过滤与诊断。

#[cfg(test)]
mod support;

#[cfg(test)]
#[path = "document_test.rs"]
mod document_tests;

#[cfg(test)]
#[path = "parameters_test.rs"]
mod parameters_tests;

#[cfg(test)]
#[path = "tags_test.rs"]
mod tags_tests;
