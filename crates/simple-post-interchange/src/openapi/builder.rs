//! 文档的装配：`Pending` 是显式工作栈，`Builder` 累积 paths / components / tags。

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{json, Map, Value};
use simple_post_core::domain::{ApiKeyLocation, AuthConfig, AuthType};

use crate::diagnostics::Diagnostic;
use crate::ir::{IrCollection, IrEndpoint, IrModule, IrScripts, IrServer};
use crate::spec::{escape_pointer_token, PATH_METHODS};

use super::{
    component_key, document_tags, media_type_of, normalize_path_template, operation_id, parameters,
    path_key_is_expressible,
};

/// 一层待写的内容。`servers` 随栈一起下潜：URL 是 per 模块的，分组里的 endpoint
/// 不能借用兄弟模块的那一份。
pub(super) struct Pending<'a> {
    pub(super) servers: &'a [IrServer],
    pub(super) endpoints: &'a [IrEndpoint],
    pub(super) collections: &'a [IrCollection],
    /// 本层 endpoint 最外层集合的名字，即导出时当 tag 用的那个。模块直接挂着的
    /// endpoint 没有它，于是导出后不带 tag。
    pub(super) group: Option<&'a str>,
}

impl<'a> Pending<'a> {
    pub(super) fn module(m: &'a IrModule) -> Self {
        Self {
            servers: &m.servers,
            endpoints: &m.endpoints,
            collections: &m.collections,
            group: None,
        }
    }

    /// `group` 是**最外层**那个集合的名字，从栈顶继承而来：已经在一层集合里时
    /// 继续沿用它，不被更内层的名字覆盖。
    pub(super) fn collection(
        c: &'a IrCollection,
        servers: &'a [IrServer],
        group: Option<&'a str>,
    ) -> Self {
        let group = group.or(Some(c.name.as_str()));
        Self {
            servers,
            endpoints: &c.endpoints,
            collections: &c.collections,
            group,
        }
    }
}

#[derive(Default)]
pub(super) struct Builder {
    pub(super) paths: Map<String, Value>,
    pub(super) schemes: Map<String, Value>,
    pub(super) schemas: Map<String, Value>,
    /// `path + verb` → 已写入的端点名。同一对只能有一个 operation，故第二次撞上时
    /// 丢弃而不是覆盖：覆盖没有任何人看得见。
    taken: HashMap<String, String>,
    /// 已用过的 `operationId`。规范要求它在整份文档里唯一，而「动词 + 路径」的映射
    /// 不是单射，故撞上时给后来的那个编号。
    used_operation_ids: HashSet<String>,
    /// 手写 schema 的去重账本（按原文），同名 schema 只写一份。
    schema_seen: HashMap<String, String>,
    pub(super) tags: BTreeSet<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

impl Builder {
    pub(super) fn operation(&mut self, ep: &IrEndpoint, servers: &[IrServer], group: Option<&str>) {
        let verb = ep.method.as_str().to_ascii_lowercase();
        let pointer = format!("/paths/{}/{}", escape_pointer_token(&ep.path), verb);
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

        // `{{name}}` → `{name}`：两种方言的路径模板讲的是同一件事，换完才谈得上
        // 「这个键合不合规范」。指针仍指向原 path——那才是用户在预览里看到的那一行。
        let path = normalize_path_template(&ep.path);
        if !path_key_is_expressible(&path) {
            self.diagnostics.push(
                Diagnostic::warn(
                    &pointer,
                    "OpenAPI 的 paths 键必须以 / 开头，这条地址里没有一个可导出的路径",
                )
                .dropping(format!("{} {}", ep.method.as_str(), ep.path)),
            );
            return;
        }

        let slot = format!("{} {}", verb, path);
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
        // OpenAPI 没有脚本的对应构造。IR 里有而文档表达不了的，必须回诊断——
        // 「导出成功」四个字会让用户看不见自己少了什么。
        self.report_scripts(&ep.scripts, &pointer, "操作");
        // 由 `动词 + 路径` 拼，不取接口名：改名是用户随手做的事，而 operationId 是
        // 代码生成器拿来给方法取名的，它变了等于换了一个方法。`动词 + 路径` 只在
        // 同一路径同一动词下唯一——那正是上面 `taken` 守住的那一格。
        //
        // 但那**一格**之外还有撞名（`/a-b` 与 `/a_b`），故落笔前再过一次文档内的
        // 唯一性检查：规范要求 `operationId` 在文档内唯一，撞名会让代码生成器生成
        // 两个同名方法。
        op.insert(
            "operationId".into(),
            json!(self.claim_operation_id(&operation_id(&verb, &path))),
        );
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

        let params = parameters(ep, &path, &pointer, &mut self.diagnostics);
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }

        // 请求体的 schema。端点自己带了就 `$ref` 到 `components.schemas` 里那一份；
        // 没带就只声明「这是一个 object」——字段**不猜**。
        //
        // `collect_schema` 必须**先**跑：它决定这份 schema 在 components 里叫什么，
        // `$ref` 要指的就是那个名字。先写 requestBody 再收集的话，收集到的定义就
        // 成了没人引用的孤儿：`components.schemas` 里躺着一堆 `Schema1`…，而每个
        // requestBody 都宣称自己是一个 object。
        let schema_name = self.collect_schema(ep, &pointer);
        if let Some(media) = media_type_of(&ep.body.mode) {
            let schema = match &schema_name {
                Some(name) => json!({ "$ref": format!("#/components/schemas/{name}") }),
                None => json!({ "type": "object" }),
            };
            op.insert(
                "requestBody".into(),
                json!({ "content": { media: { "schema": schema } } }),
            );
        }

        // 合成默认：响应历史可能被截断，猜出来的 schema 看不出不可靠，比不猜更糟。
        op.insert(
            "responses".into(),
            json!({ "200": { "description": "OK" } }),
        );

        match self.register_scheme(&ep.auth, &pointer) {
            Some(name) => {
                let mut requirement = Map::new();
                requirement.insert(name, Value::Array(Vec::new()));
                op.insert(
                    "security".into(),
                    Value::Array(vec![Value::Object(requirement)]),
                );
            }
            // 显式写「这条不要求任何凭据」。`register_scheme` 只对 `AuthType::None`
            // 返回 None，所以这里写空数组是**真话**而不是「表达不出来」——
            // OAuth2 会拿到一条 flows 为空的 scheme，不会落到这个分支。
            //
            // 顺带消掉 `security-defined`：没有它，读者得自己推断「既没有根级
            // security、这条也没有」等于不需要凭据。
            None => {
                op.insert("security".into(), Value::Array(Vec::new()));
            }
        }
        let tags = document_tags(ep, group);
        if !tags.is_empty() {
            for t in &tags {
                self.tags.insert(t.clone());
            }
            op.insert("tags".into(), json!(tags));
        }

        let item = self
            .paths
            .entry(path)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = item.as_object_mut() {
            map.insert(verb, Value::Object(op));
        }
    }

    /// 端点 auth → 一条 securityScheme，返回它的名字。同名只写一份。
    ///
    /// 名字由**内容**决定（apiKey 用键名），否则两个用不同键的端点会互相覆盖，
    /// 而丢掉的那个键名没有任何人看得见。
    pub(super) fn register_scheme(&mut self, auth: &AuthConfig, pointer: &str) -> Option<String> {
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

    /// 手写的 `body.schema` 透传到 `components.schemas`，返回它在 components 里的名字。
    ///
    /// 它是一个 `Option<String>`（JSON 文本）且没有名字，故按出现顺序编号。解析不成
    /// 合法 JSON Schema 就忽略 + `Info`，**不报错**：UI 从不写这个字段，一份畸形
    /// schema 不该让整次导出失败。
    ///
    /// **返回名字而不只是收集**：调用方拿它写 `requestBody` 里的 `$ref`。收集了却
    /// 不被引用是三种选择里最差的一种——`components.schemas` 里躺着一堆没人引用的
    /// 定义（读文档的人看到 `Schema1`…`Schema9` 会以为它们有意义），而每个
    /// requestBody 都在宣称一个错的请求体形状。
    pub(super) fn collect_schema(&mut self, ep: &IrEndpoint, pointer: &str) -> Option<String> {
        let raw = ep.body.schema.as_ref().filter(|s| !s.trim().is_empty())?;
        match serde_json::from_str::<Value>(raw) {
            Ok(v) if v.is_object() => {
                let text = v.to_string();
                if let Some(name) = self.schema_seen.get(&text) {
                    // 同一份 schema 只写一份，两个端点引同一个名字。
                    return Some(name.clone());
                }
                let name = format!("Schema{}", self.schemas.len() + 1);
                self.schemas.insert(name.clone(), v);
                self.schema_seen.insert(text, name.clone());
                Some(name)
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::info(
                        pointer,
                        "请求体上这个 schema 不是合法的 JSON Schema 对象，没有写进 components.schemas",
                    )
                    .dropping(raw.clone()),
                );
                None
            }
        }
    }

    /// OpenAPI 没有脚本的对应构造，故 IR 里有脚本就要说出来。
    ///
    /// 「导出成功」四个字会让用户看不见自己少了什么。`what` 是这个节点在文档里的
    /// 身份（操作 / 集合 / 模块），指针指到它在文档或树里的位置。
    pub(super) fn report_scripts(&mut self, scripts: &IrScripts, pointer: &str, what: &str) {
        if scripts.is_empty() {
            return;
        }
        self.diagnostics.push(Diagnostic::warn(
            pointer,
            format!("这个{what}上配了脚本，OpenAPI 没有对应构造，这两段脚本没有写进文档"),
        ));
    }

    /// 给这个 operation 找一个文档内唯一的 `operationId`。
    ///
    /// [`operation_id`] 的映射不是单射：`/a-b` 与 `/a_b` 都映成同一个名字，而规范
    /// 要求 `operationId` 在整份文档里唯一。撞上时给后来的那个加编号后缀。
    fn claim_operation_id(&mut self, base: &str) -> String {
        let mut candidate = base.to_string();
        let mut n = 2u32;
        while self.used_operation_ids.contains(&candidate) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        self.used_operation_ids.insert(candidate.clone());
        candidate
    }
}
