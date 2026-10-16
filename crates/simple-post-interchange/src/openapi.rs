//! IR → OpenAPI 3.0 文档。
//!
//! 这条边是**投影**：IR 里没有的构造文档里就不写。IR 里有而 OpenAPI 表达不了的，
//! 必须回诊断（见 [`Document::diagnostics`]）——「导出成功」四个字会让用户看不见
//! 自己少了什么。
//!
//! 两个约定是决策，不是实现细节：
//!
//! - **`responses` 合成默认，不猜。** `StoredResponse` 存的是实际响应，里面没有响应
//!   schema；`RequestBody.schema` UI 从不写。故每个 operation 输出一个 `200: OK`。
//! - **`servers` 逐模块取，不回退。** 该模块没配就是没有 URL：不借用别的模块的，
//!   也不落回项目 ★。

use std::collections::{BTreeSet, HashMap};

use serde_json::{json, Map, Value};
use simple_post_core::domain::{ApiKeyLocation, AuthConfig, AuthType, BodyMode};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrCollection, IrEndpoint, IrModule, IrServer};

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

/// OpenAPI 3.0 `paths` 的固定动词键集。集合外的动词塞进去会让整份文档不合法，
/// 故导出方必须挡住它，而不是「尽量映射」。
const PATH_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// `IrEndpoint.body.mode` → OpenAPI media type。`BodyMode::None` 没有 media type，
/// 故返回 `None` 且不写 `requestBody`。
fn media_type_of(mode: &BodyMode) -> Option<&'static str> {
    match mode {
        BodyMode::None => None,
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Multipart => Some("multipart/form-data"),
        BodyMode::Raw => Some("application/octet-stream"),
    }
}

/// 一层待写的内容。`servers` 随栈一起下潜：URL 是 per 模块的，分组里的 endpoint
/// 不能借用兄弟模块的那一份。
struct Pending<'a> {
    servers: &'a [IrServer],
    endpoints: &'a [IrEndpoint],
    collections: &'a [IrCollection],
}

impl<'a> Pending<'a> {
    fn module(m: &'a IrModule) -> Self {
        Self {
            servers: &m.servers,
            endpoints: &m.endpoints,
            collections: &m.collections,
        }
    }

    fn collection(c: &'a IrCollection, servers: &'a [IrServer]) -> Self {
        Self {
            servers,
            endpoints: &c.endpoints,
            collections: &c.collections,
        }
    }
}

#[derive(Default)]
struct Builder {
    paths: Map<String, Value>,
    schemes: Map<String, Value>,
    schemas: Map<String, Value>,
    /// `path + verb` → 已写入的端点名。同一对只能有一个 operation，故第二次撞上时
    /// 丢弃而不是覆盖：覆盖没有任何人看得见。
    taken: HashMap<String, String>,
    /// 手写 schema 的去重账本（按原文），同名 schema 只写一份。
    schema_seen: HashMap<String, String>,
    tags: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
}

impl Builder {
    fn operation(&mut self, ep: &IrEndpoint, servers: &[IrServer]) {
        let verb = ep.method.as_str().to_ascii_lowercase();
        let pointer = format!("/paths/{}/{}", escape(&ep.path), verb);
        if !PATH_METHODS.contains(&verb.as_str()) {
            self.diagnostics.push(
                Diagnostic::warn(
                    pointer,
                    "OpenAPI 3.0 的 paths 只有固定动词键集，该动词无法导出，整个操作被跳过",
                )
                .dropping(format!("{} {}", ep.method.as_str(), ep.path)),
            );
            return;
        }

        let slot = format!("{} {}", verb, ep.path);
        if let Some(kept) = self.taken.get(&slot) {
            self.diagnostics.push(
                Diagnostic::warn(&pointer, "同一路径与动词下已经有另一个操作，只能留一个")
                    .dropping(format!("{} {}", ep.method.as_str(), ep.path)),
            );
            self.diagnostics.push(Diagnostic::info(
                &pointer,
                format!("被覆盖掉的是操作「{kept}」"),
            ));
            return;
        }
        self.taken.insert(slot, ep.name.clone());

        let mut op = Map::new();
        if !ep.name.is_empty() {
            op.insert("summary".into(), json!(ep.name));
        }
        if let Some(d) = &ep.description {
            op.insert("description".into(), json!(d));
        }
        // 逐模块取，不回退：该模块没配就没有这一项。
        if !servers.is_empty() {
            op.insert(
                "servers".into(),
                Value::Array(servers.iter().map(|s| json!({ "url": s.url })).collect()),
            );
        }

        let params: Vec<Value> = ep
            .params
            .iter()
            .filter(|p| p.enabled)
            // 值的实际形态不猜：启用即视为必填，schema 一律按 string 写。
            .map(|p| {
                json!({ "name": p.key, "in": "query", "required": true, "schema": { "type": "string" } })
            })
            .collect();
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }

        if let Some(media) = media_type_of(&ep.body.mode) {
            op.insert(
                "requestBody".into(),
                json!({ "content": { media: { "schema": { "type": "object" } } } }),
            );
        }
        self.collect_schema(ep, &pointer);

        // 合成默认：响应历史可能被截断，猜出来的 schema 看不出不可靠，比不猜更糟。
        op.insert(
            "responses".into(),
            json!({ "200": { "description": "OK" } }),
        );

        if let Some(name) = self.register_scheme(&ep.auth, &pointer) {
            let mut requirement = Map::new();
            requirement.insert(name, Value::Array(Vec::new()));
            op.insert(
                "security".into(),
                Value::Array(vec![Value::Object(requirement)]),
            );
        }
        if !ep.tags.is_empty() {
            for t in &ep.tags {
                self.tags.insert(t.clone());
            }
            op.insert("tags".into(), json!(ep.tags));
        }

        let item = self
            .paths
            .entry(ep.path.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = item.as_object_mut() {
            map.insert(verb, Value::Object(op));
        }
    }

    /// 端点 auth → 一条 securityScheme，返回它的名字。同名只写一份。
    ///
    /// 名字由**内容**决定（apiKey 用键名），否则两个用不同键的端点会互相覆盖，
    /// 而丢掉的那个键名没有任何人看得见。
    fn register_scheme(&mut self, auth: &AuthConfig, pointer: &str) -> Option<String> {
        let (name, value) = match auth.kind {
            AuthType::None => return None,
            AuthType::Bearer => (
                "bearerAuth".to_string(),
                json!({ "type": "http", "scheme": "bearer" }),
            ),
            AuthType::Basic => (
                "basicAuth".to_string(),
                json!({ "type": "http", "scheme": "basic" }),
            ),
            AuthType::Oauth2 => {
                self.diagnostics.push(Diagnostic::warn(
                    pointer,
                    "本仓库的 OAuth2 配置与 OpenAPI 的 flows 结构对不上，flows 留空",
                ));
                (
                    "oauth2Auth".to_string(),
                    json!({ "type": "oauth2", "flows": {} }),
                )
            }
            AuthType::Apikey => {
                let key = auth
                    .apikey
                    .as_ref()
                    .map(|a| a.key.trim().to_string())
                    .unwrap_or_default();
                let name = if key.is_empty() {
                    "apiKeyAuth".to_string()
                } else {
                    component_key(&key)
                };
                let loc = match auth
                    .apikey
                    .as_ref()
                    .map(|a| a.in_location)
                    .unwrap_or_default()
                {
                    ApiKeyLocation::Header => "header",
                    ApiKeyLocation::Query => "query",
                };
                (name, json!({ "type": "apiKey", "name": key, "in": loc }))
            }
        };
        self.schemes.entry(name.clone()).or_insert(value);
        Some(name)
    }

    /// 手写的 `body.schema` 透传到 `components.schemas`。
    ///
    /// 它是一个 `Option<String>`（JSON 文本）且没有名字，故按出现顺序编号。解析不成
    /// 合法 JSON Schema 就忽略 + `Info`，**不报错**：UI 从不写这个字段，一份畸形
    /// schema 不该让整次导出失败。
    fn collect_schema(&mut self, ep: &IrEndpoint, pointer: &str) {
        let Some(raw) = ep.body.schema.as_ref().filter(|s| !s.trim().is_empty()) else {
            return;
        };
        match serde_json::from_str::<Value>(raw) {
            Ok(v) if v.is_object() => {
                let text = v.to_string();
                if !self.schema_seen.contains_key(&text) {
                    let name = format!("Schema{}", self.schemas.len() + 1);
                    self.schemas.insert(name.clone(), v);
                    self.schema_seen.insert(text, name);
                }
            }
            _ => self.diagnostics.push(
                Diagnostic::info(
                    pointer,
                    "请求体上这个 schema 不是合法的 JSON Schema 对象，没有写进 components.schemas",
                )
                .dropping(raw.clone()),
            ),
        }
    }
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

/// JSON Pointer 转义：`/` 与 `~` 不能原样出现在指针里。
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// 把一份 [`Exchange`] 落成 OpenAPI 3.0 文档。
pub fn to_document(ex: &Exchange, format: DocFormat) -> Result<Document, AppError> {
    let mut b = Builder::default();

    // 显式工作栈而非递归：IR 的 collection 嵌套深度无上限。
    let mut stack: Vec<Pending> = ex.modules.iter().rev().map(Pending::module).collect();
    while let Some(p) = stack.pop() {
        for ep in p.endpoints {
            b.operation(ep, p.servers);
        }
        stack.extend(
            p.collections
                .iter()
                .rev()
                .map(|c| Pending::collection(c, p.servers)),
        );
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
#[path = "openapi_test.rs"]
mod tests;
