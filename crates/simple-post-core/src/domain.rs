//! 与 `src/types/domain.ts` 1:1 镜像的共享领域类型。
//!
//! 这些结构体是跨 Tauri IPC 边界的线格式，字段名一律由 `#[serde(rename_all = "camelCase")]`
//! 一次导出，React 侧无需翻译。
//!
//! 持久化层（`crate::db`）把复杂嵌套字段（KeyValue[]、RequestBody、AuthConfig）序列化成
//! JSON TEXT 列，往返见 `db/endpoint_details.rs`。
//!
//! **本文件不允许出现逐字段 `#[serde(rename)]`。** Rust 字段名**就是**线名。逐字段 rename 是
//! 字段名的第二处真相，也正是两种语言开始分叉的缝：TypeScript 侧 `in` 是保留字，写不出来。
//! 线名确需不同于 Rust 名时，扩展 Rust 名——故用 `in_location` 而非 `in`；加下划线后缀没用，
//! `in_` camelCase 之后又变回 `in`。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

// HTTP 方法

/// TS 侧 `HttpMethod` 的镜像。序列化为渲染层已在用的大写动词（`"GET"`、`"POST"`…）；
/// 未知动词落到 `Custom(String)`，使 UI 的前向兼容写入不会在 Rust 侧炸掉。
///
/// `Deserialize` 手写，好让本类型只有**一个**解析器：派生实现会把 `"get"` 收成
/// `Custom("get")` 并原样存库，于是 IPC 写入与 MCP 调用同一个动词会存出两种表示。
/// 一切都走 [`parse_http_method`]。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    /// 标准列表以外的动词（如 `PROPFIND`、`LOCK`）。序列化为裸字符串：枚举的 `UPPERCASE`
    /// 规则会重命名上面的单元变体，而不加 `untagged` 这个会变成 `{"CUSTOM":"PROPFIND"}`。
    /// 配套的**读**侧是下面的手写 `Deserialize`，它不需要这个属性。
    #[serde(untagged)]
    Custom(String),
}

impl<'de> serde::Deserialize<'de> for HttpMethod {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        parse_http_method(&raw).map_err(serde::de::Error::custom)
    }
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Custom(v) => v.as_str(),
        }
    }
}

/// RFC 9110 `token`：方法允许出现的全部字符。空格、中日韩字符、控制字节在此拒绝，
/// 否则会一路带到 `reqwest`，在发送那一刻才变成一个不透明的传输错误。
fn is_method_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// 每个 MCP 工具、请求执行路径**以及** `HttpMethod` 的 `Deserialize` 共用的解析器。
/// 已知动词大小写不敏感地映射；其余 RFC 9110 token（如 `PROPFIND`、`LOCK`）原样流成
/// `HttpMethod::Custom`，使将来/冷门的 HTTP 动词不丢数据。空串、含空白、非 token 的输入
/// 以 `AppError::Validation` 拒绝，好让调用方（通常是 LLM）拿到一条说明问题的消息。
pub fn parse_http_method(m: &str) -> Result<HttpMethod, crate::error::AppError> {
    use crate::error::AppError;
    let trimmed = m.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("method must not be empty".into()));
    }
    if !is_method_token(trimmed) {
        return Err(AppError::Validation(format!(
            "method {trimmed:?} is not a valid HTTP token"
        )));
    }
    Ok(match trimmed.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        other => HttpMethod::Custom(other.into()),
    })
}

// 键值对

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub id: String,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// body

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BodyMode {
    #[default]
    None,
    Json,
    Form,
    Raw,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    pub mode: BodyMode,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

// auth

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    #[default]
    None,
    Bearer,
    Basic,
    Apikey,
}

/// apikey 挂在哪。线上为 `inLocation: "header" | "query"`；
/// 为何不能用显然的 `in` 见模块文档。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
    /// 序列化为 `inLocation`。`in` 在 TypeScript 是保留字；`in_` 也不行——
    /// `rename_all = "camelCase"` 会把 `in_` 变回线上的 `in`，两种语言照样对不上。见模块文档。
    pub in_location: ApiKeyLocation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    /// 用的是哪种方案。线上叫 `kind`，因为这里字段就叫 `kind`：`type` 在 Rust 里同样是
    /// 关键字，用 serde `rename` 遮掩只会在 Tauri command 参数与 REST body 之间重新撕开一道缝。
    pub kind: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apikey: Option<ApiKeyAuth>,
}

// base url

/// 项目服务表里的一条具名服务。
///
/// 服务表是**项目级**的：每个模块都能看到全部服务。故本结构体没有模块字段，也没有逐行
/// 默认标记——于是一个项目里两个服务不可能同名，endpoint 的 `baseUrlDefinitionId` 在整棵
/// 树上无歧义。表中那两个为已下线的「模块持有」模型服务的列（`module_id` / `is_default`）
/// 仍在表里但已被中和：删列 = 重建表 = bump `SCHEMA_VERSION` = 删掉用户的库。
///
/// 本行只指认服务，从不持有 URL。(服务 × 模块 × 环境) 的 URL 在
/// [`Environment::module_base_urls`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefinition {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// 资源树：module / collection / endpoint 各一行，endpoint 的请求配置拆到 `EndpointDetail`。

/// [`ResourceNode`] 的种类。项目不是节点——项目是隐式的根上下文，由 `root_id` 指名。
///
/// 这个集合刻意封闭：`nodes.type` 上有配套的 CHECK 约束，`parse` 对其余值一律拒绝而不
/// 凭空造变体，故未知值永远到不了那些会把它当成别的东西的 match 分支。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceNodeType {
    Module,
    Collection,
    Endpoint,
}

impl ResourceNodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceNodeType::Module => "module",
            ResourceNodeType::Collection => "collection",
            ResourceNodeType::Endpoint => "endpoint",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "module" => Some(ResourceNodeType::Module),
            "collection" => Some(ResourceNodeType::Collection),
            "endpoint" => Some(ResourceNodeType::Endpoint),
            _ => None,
        }
    }

    /// 本类型的节点能否直接挂在 `parent` 类型的节点下。
    ///
    /// SQLite 无法在单一多态 `parent_id` 上表达它——CHECK 约束读不到别的行——故矩阵写在
    /// 这里，并由 `db::nodes` 在每次写入时执行，`schema.sql` 另有配套触发器。
    ///
    /// endpoint 的父刻意是两个类型：模块直接收 endpoint，免得「一个 endpoint」这种最简场景
    /// 先得为此建一个用完即弃的 collection；collection 同样收 endpoint，也继续收子 collection。
    pub fn may_under(self, parent: ResourceNodeType) -> bool {
        match self {
            // 模块的父是项目，不是节点。调用方以 `parent_id = NULL` 表达，故「无父」是唯一合法情形。
            ResourceNodeType::Module => false,
            // collection 要么开在模块下，要么嵌在另一个 collection 下——嵌套深度无上限，
            // 故树渲染器是递归的。
            ResourceNodeType::Collection => matches!(
                parent,
                ResourceNodeType::Module | ResourceNodeType::Collection
            ),
            // collection 用来把 endpoint 与同级分组；模块则直接收下不需要独立文件夹的那个。
            ResourceNodeType::Endpoint => matches!(
                parent,
                ResourceNodeType::Module | ResourceNodeType::Collection
            ),
        }
    }
}

/// 资源树的一行：只有结构与身份。
///
/// endpoint 的请求配置在 [`EndpointDetail`] 里，按 `node_id` 关联。拆开不是为了整洁：
/// `method` / `url` / `tags` 对模块毫无意义，折叠进来会让每个模块和 collection 行永远
/// 带着空列。
///
/// `src/types/domain.ts` 侧的镜像多一个字段：渲染器的折叠/展开态 `expanded?: boolean`。
/// 此处刻意不要——它既不落库也不属于节点身份。缺它也安全：本结构体忽略未知键，渲染层
/// 带 `expanded` 回传会被直接丢弃。用 `#[serde(deny_unknown_fields)]` 堵这个口是错的：
/// 它会连该字段一并拒绝，也会破坏 [`EndpointResource`] 的 `#[serde(flatten)]`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNode {
    pub id: String,
    /// 所属项目。反范式化：整棵树因此是一次走索引的读，「这属于哪个项目」——逐项目权限门的
    /// 第一步——也变成主键命中，而不必沿父链上溯。
    pub root_id: String,
    /// `None` 表示模块，其父是项目。
    pub parent_id: Option<String>,
    /// 这一行是三种里的哪一种。线上叫 `kind`，理由同 [`AuthConfig::kind`]：`type` 是 Rust
    /// 关键字，把它在线上叫 `type` 只会重开 Tauri command 参数与 REST body 之间的缝。
    pub kind: ResourceNodeType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i64,
    #[serde(default = "now_ms")]
    pub created_at: i64,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNodePatch {
    /// 改父。`None` 保持原父不动；显式 `Some(None)` 把节点提到项目顶层，这只对模块合法。
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
}

impl ResourceNodePatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// endpoint 的请求配置。按 `node_id` 与 [`ResourceNode`] 关联；树的列刻意不出现在这里，
/// 好让它们各自只有一个归属。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetail {
    pub node_id: String,
    pub method: HttpMethod,
    /// 只有路径——host 在发送时从当前环境的 `moduleBaseUrls` 解析，故同一 endpoint 可在
    /// dev / staging / prod 之间改指向而无需重写。
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// 该 endpoint 解析所依据的服务。`None` 表示「用项目的 ★」
    /// （`Project::default_base_url_definition_id`），这与指名某个服务不是一回事：区别要到
    /// 读取该值的解析步骤才显形，故此处保持缺席，而不是在写入时就填上默认服务的 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetailPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<KeyValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<KeyValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl EndpointDetailPatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// `value` 是否不含任何会上线的字段。
///
/// 「有没有东西要写」由「能否序列化成空对象」回答，而不是自己再列一遍字段：每个 patch 字段
/// 都带 `skip_serializing_if = "Option::is_none"`，故该属性**就是**「缺席即不碰此列」的编码，
/// 判定从它推导出来就只有一处真相，新增字段不会与手写字段表失配。三态情形随之免费得到：
/// `Some(None)`（清空该列）序列化为显式 `null`，键存在，patch 正确地*不*算空。
///
/// 这些类型不会序列化失败（字段只有标量、`Option`、`Vec`，或本身 `Serialize` 的嵌套结构），
/// 但兜底取 `false` 而非 `true` 是刻意的：偏向「非空」意味着意外失败会让写入照常发生、
/// 真实错误在 DB 层暴露，而不是被吞成一次静默的空操作。
fn serialises_to_empty_object<T: Serialize>(value: &T) -> bool {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_object().map(|m| m.is_empty()))
        .unwrap_or(false)
}

/// 树与编辑器共同看到的 endpoint：节点结构 + 请求配置。
/// 与 [`ResourceNode`] 分开，好让树的读取可以完全跳过 `endpoint_details`——树只渲染
/// name / type / sort_order，为此对每个节点都 join 详情表，等于读一组多数行根本没有的列。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointResource {
    #[serde(flatten)]
    pub node: ResourceNode,
    pub detail: EndpointDetail,
}

// project

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub base_url_definitions: Vec<BaseUrlDefinition>,
    /// 项目的 ★ 服务。未指名服务的 endpoint 按它解析，故此处为 `None` 的项目发出这类请求时
    /// 没有 base URL，而不是借用别的项目或别的模块的。新建项目就是 NULL：不播种，因为用户
    /// 没点名的服务，是用户随后还得去删的服务。
    ///
    /// 刻意**不**加 `skip_serializing_if`：「本项目没有默认 baseUrl」是项目做出的决定，不是
    /// 数据缺席；TypeScript 镜像把它声明为必填但可空（`defaultBaseUrlDefinitionId:
    /// string | null`），发 `null` 两边才讲同一个故事，省掉键则让一个已声明的契约字段以
    /// `undefined` 到达。本结构体其余可选字段在镜像里确实可选，故保留 skip。
    #[serde(default)]
    pub default_base_url_definition_id: Option<String>,
    #[serde(default = "default_project_color")]
    pub color: String,
    /// 服务端时间戳；调用方（如 REST 的 `POST /api/projects` handler）不给时取 `now_ms()`。
    /// Tauri command 一律显式设置，故对它们而言该默认值是惰性的。
    #[serde(default = "now_ms")]
    pub created_at: i64,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definitions: Option<Vec<BaseUrlDefinition>>,
    /// 项目的 ★。`Some(None)` 清空它，这是真实状态而非省略：渲染器在列出服务的同一个菜单里
    /// 提供「无默认服务」，而未指名服务的 endpoint 于是解析不到任何 base URL。
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url_definition_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl ProjectPatch {
    /// 尚无写路径调用；存在是为了让下一个调用方不必自己手写字段表。见 [`serialises_to_empty_object`]。
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

// environment

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// **已废弃——仅为线兼容而读，无人写入。**
    ///
    /// 它曾是 `def_id -> url`，即 URL 还不关心 endpoint 挂在哪个模块时的值层。现役层是
    /// [`Self::module_base_urls`]；`db::migrate` 已把每个值搬过去，且是本 crate 里仅剩的
    /// 该列读者。
    ///
    /// 留在结构体上，好让旧客户端的载荷仍能反序列化（镜像声明为可选），但
    /// `db::environments::update` 忽略它。静默丢弃是代价，此处即是记录。
    #[serde(default)]
    pub base_urls: HashMap<String, String>,
    /// `def_id -> module_id -> url`。**URL 只存在于这一层。**
    ///
    /// 每个单元格独立：缺项表示「此模块对此服务无地址」，解析到此为止，不会回落到环境级
    /// map 或别的模块的值。故该 map 之所以按模块做键：两个模块完全可以把同一服务指向不同
    /// host，而空单元格是一个真实答案，不是一个洞。
    #[serde(default)]
    pub module_base_urls: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 随 [`Environment::base_urls`] 一起废弃：只收下以让旧载荷仍能反序列化，从不写入——
    /// 该列已死。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_urls: Option<HashMap<String, String>>,
    /// 整张 `module_base_urls` map 替换。`db::environments::update` 以一次原子 `UPDATE`
    /// 写入该 JSON 列，故读者永远看不到半个 map。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_base_urls: Option<HashMap<String, HashMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

impl EnvironmentPatch {
    /// 尚无写路径调用；存在是为了让下一个调用方不必自己手写字段表。见 [`serialises_to_empty_object`]。
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

// agent 权限

/// 逐项目的 MCP 权限。`Read < ReadCall < Full`；序关系本身在
/// [`crate::permissions::permission_at_least`]。
///
/// 用 `snake_case` 而非 `lowercase`：后者把 `ReadCall` 映射成 `"readcall"`，一个既不匹配
/// [`Self::as_str`]（`db::permissions` 写的、`parse` 读的），也不匹配 `agent_permissions`
/// 的 CHECK 约束、更不匹配 TypeScript 侧 `AgentPermission` 联合类型的第三种拼法。
/// `snake_case` 让四处一致，且与其兄弟 [`crate::permissions::CapabilityPolicy`] 相同。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    #[default]
    ReadCall,
    Full,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::ReadCall => "read_call",
            Permission::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Permission::Read),
            "read_call" => Some(Permission::ReadCall),
            "full" => Some(Permission::Full),
            _ => None,
        }
    }
}

// 请求设置

/// 出站请求旋钮。单例，作为 JSON blob 存在 `preferences` KV 表里，而非自成一表。
///
/// 容器级 `#[serde(default)]` 的分量在**读**侧而非写侧：存下的 blob 是某个更早构建写的，
/// 故此日新增第四个字段时，每个既有安装的行都能缺它反序列化——否则
/// `request_settings_get` 会让它们全部失败。代价落在写侧：`PUT /api/request_settings`
/// 是整体替换，缺字段的请求体会把该字段重置为默认值而不是被拒。对一个默认值写在
/// [`Default`] 上的设置单例，这更有用；该路由自己的注释也这么写。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestSettings {
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            follow_redirects: true,
            max_response_size_kb: 5120,
        }
    }
}

// MCP HTTP 监听配置

/// MCP HTTP 守护进程监听在哪、应答谁。
///
/// 设置单例，作为 JSON blob 存在 `preferences` 的
/// [`crate::db::preferences::keys::MCP_HTTP_CONFIG`] 之下——与所有其他结构化设置同属
/// 「preferences 袋子装 JSON 值」这条规则。
///
/// 容器级 `#[serde(default)]` 的分量在**读**侧，理由同 [`RequestSettings`]：存下的 blob 是
/// 更早构建写的，后来新增的字段不能让每个既有安装的行解析失败。代价落在写侧——缺字段会
/// 把该字段重置为默认值而不是被拒——对设置单例而言这更有用。
///
/// 与安全相关的是 `allow_remote` 加那两张允许表，且它们**结构上 fail-closed**：默认值
/// （`allow_remote: false`，两表皆空）只接受 loopback `Host`，且完全不接受任何 `Origin`。
/// `validate` 拒绝唯一那个自相矛盾的组合——非 loopback 绑定却不开远程访问，那会监听在
/// 局域网上而守卫挡掉所有真正从那里到达的请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct McpHttpConfig {
    /// 绑定地址。`127.0.0.1` 即守护进程仅本机可达。
    pub host: String,
    /// 监听端口。`0` 表示「由 OS 分配」——守护进程自己选定的端口随后经 `mcp.port` 记录公布。
    pub port: u16,
    /// JSON-RPC 端点的挂载路径。必须以 `/` 开头。
    pub path: String,
    /// 非 loopback 访问的总开关。`false`（默认）表示 `Host` 守卫只接受 loopback 名字。
    pub allow_remote: bool,
    /// 开启 `allow_remote` 后接受的 `Host` 值。裸 `"*"` 表示任意。空表示**不接受**任何
    /// 非 loopback host——不是「全都接受」。
    pub allowed_hosts: Vec<String>,
    /// 接受的 `Origin` 值。AI 客户端（curl、Claude Code）根本不发 `Origin`，故永不被此拦；
    /// 确实发了的浏览器式请求必须匹配。裸 `"*"` 表示任意。
    pub allowed_origins: Vec<String>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_MCP_HOST.to_string(),
            port: 0,
            path: DEFAULT_MCP_PATH.to_string(),
            allow_remote: false,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
        }
    }
}

/// Default bind address for the MCP HTTP daemon — loopback only.
pub const DEFAULT_MCP_HOST: &str = "127.0.0.1";
/// Default mount path for the MCP JSON-RPC endpoint.
pub const DEFAULT_MCP_PATH: &str = "/mcp";

impl McpHttpConfig {
    /// axum 路由器想要的挂载路径形态：恒有前导 `/`，恒无尾随 `/`。`"/"`、`""` 与缺斜杠一律
    /// 收敛到 [`DEFAULT_MCP_PATH`]，而不是产出一个把设置面板里看得见的东西全都 404 的路由器。
    pub fn normalized_path(&self) -> String {
        let raw = self.path.trim();
        let trimmed = raw.trim_end_matches('/');
        if trimmed.is_empty() {
            return DEFAULT_MCP_PATH.to_string();
        }
        if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        }
    }

    /// 拒绝无法自圆其说的配置。
    /// 每次写入都调用（Tauri command 与 REST 路由皆然），故自相矛盾的组合永远到不了
    /// `daemon_cli_args` 生成时读取的那一行 `preferences`。
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        use crate::error::AppError;

        if self.host.trim().is_empty() {
            return Err(AppError::Validation("host must not be empty".into()));
        }
        // 先归一化再检查，好让 `/mcp/` 与 `/mcp` 不会双双通过验证、随后在路由器上打架。
        if self.path.trim().is_empty() {
            return Err(AppError::Validation(format!(
                "path must not be empty (expected something like {DEFAULT_MCP_PATH:?})"
            )));
        }
        if !self.normalized_path().starts_with('/') {
            return Err(AppError::Validation(format!(
                "path must start with '/' (got {:?})",
                self.path
            )));
        }
        // 那个自相矛盾的组合：绑到局域网，守卫却配置为拒掉一切非 loopback——守护进程将不可达，
        // 读起来是「MCP 坏了」而不是「你的设置自相矛盾」。
        if !self.allow_remote && !is_loopback_host(&self.host) {
            return Err(AppError::Validation(format!(
                "host {:?} is not a loopback address, so allowRemote must also be \
                 enabled — otherwise the daemon listens on a network the Host guard \
                 refuses to answer",
                self.host
            )));
        }
        Ok(())
    }
}

/// `host` 是否是守卫恒接受的那几个 loopback 名字之一？
/// 由 [`McpHttpConfig::validate`] 与守护进程的 Host 守卫共用——两者必须一致，否则一份配置
/// 能通过验证、随后又把监听者挡在自己的 healthz 探针之外。
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

// API 响应（瞬态，不落 schema）。`ApiResponse` 本身仍不落库；其下是已持久化的投影，
// 是个**不同的**类型，而不是复用的同一个。

/// 响应离开 Rust 时的线形状。
///
/// 它**不是**渲染层最终持有的形状，也不是进程内形状——像这里其余类型一样跨 Tauri IPC。
/// 差别在 `headers`：此侧建模为有序对，因为响应可以合法重复某个头（多行 `Set-Cookie` 之类），
/// 而 map 表达不了。渲染层要的是普通对象，故 `src/lib/http.ts` 在到达时用
/// `tuplesToRecord` 折叠——有损，且是请求路径上唯一一处明知丢弃信息的地方；重复项按
/// 后者胜出。
/// Rust 侧没有任何东西描述适配之后的形状。`src/types/domain.ts` 的 `ApiResponse` 才是那份
/// 描述，且它刻意是另一个类型，而非声称镜像本类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub headers: Vec<[String; 2]>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// 已持久化的响应历史

/// 单个 endpoint 保留多少条响应。渲染器 `stores/slices/ui-response.ts` 的
/// `MAX_HISTORY_PER_ENDPOINT` 与之镜像——store 侧封顶使所展示的数组不增长，数据库侧封顶
/// 使 `store.db` 不增长。两者一致是锦上添花；真正给文件封顶的是数据库侧，故日后漏掉
/// store 侧的调用方也无法让库无限增长。
pub const RESPONSE_HISTORY_PER_ENDPOINT: i64 = 10;

/// 单条已存响应体的硬上限（字节）。
///
/// 历史是便利功能——要点是「这个 endpoint 十分钟前返回了什么」，不是归档。没有上限时，
/// 一个 endpoint 连返十次 50 MB 就把 500 MB 塞进用户的 `store.db` 且无法挽回，故写入时裁剪，
/// 并由 `ResponseHistoryEntry::body_truncated` 标记发生过。256 KiB 对人调试时读的 JSON 和
/// HTML 足够宽裕，把每 endpoint 最坏情况压到约 2.5 MB。
pub const RESPONSE_HISTORY_MAX_BODY_BYTES: usize = 256 * 1024;

/// 响应在**渲染层**持有的形状，也就是必须能往返出数据库的形状。
///
/// 刻意不是 [`ApiResponse`]：那个类型把 headers 建模为有序对，只为不丢重复的 `Set-Cookie`
/// 而熬过一次 IPC。响应入库时早已越过那道边界、并被 `tuplesToRecord` 折成普通对象——重复头的
/// 丢失已经发生，不是本类型的问题。此处存对子等于存渲染层已不再拥有的信息，而渲染层自己的
/// `ApiResponse` 就得多长一种形状。
/// 本类型是 `src/types/domain.ts` 里 `ApiResponse` 的 Rust 镜像，那同样是适配**之后**的类型，
/// 而非上面 [`ApiResponse`] 的镜像。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    /// 用 `BTreeMap` 而非 `HashMap`，行读回来时表头顺序才稳定——`HashMap` 每次读都打乱，
    /// 使两行相同的数据序列化出不同结果。
    pub headers: BTreeMap<String, String>,
    /// 已被写入路径裁剪到 [`RESPONSE_HISTORY_MAX_BODY_BYTES`]。
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `response_history` 的一行。
///
/// 外面包一层而不是给已存响应加宽 `id` / `createdAt`，是因为两者生命周期不同：响应是载荷，
/// 这三个是行的身份。它也让 `bodyTruncated` 紧挨它所描述的 body——把这个标志抹到
/// `StoredResponse` 上，就得在那里把它设为可选、在这里设为必填，只为一比特的信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHistoryEntry {
    pub id: String,
    pub endpoint_id: String,
    pub response: StoredResponse,
    /// `body` 已在 [`RESPONSE_HISTORY_MAX_BODY_BYTES`] 处裁剪。
    pub body_truncated: bool,
    pub created_at: i64,
}

// 辅助函数

/// 当前 unix 毫秒。写路径上所有 `*_at` 列的唯一真相，使单个事务内的值自洽。
/// `SystemTime::now()` 只在时钟早于 unix epoch 时 panic；把这当程序员错误，而非用户可恢复的故障。
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

/// 调用方未给项目颜色时的默认值。住在这里而非 `commands/`，是为了让 REST 与 Tauri 两条写路径
/// 无法分叉：`commands` 已依赖 `domain`，在那条边另一侧再放一份只会自由地各说各话。
pub fn default_project_color() -> String {
    "#3b82f6".to_string()
}

/// 新主键：`sp_` + 墙钟十六进制 + 每进程计数器，拼接而成。
///
/// 它**不是**不透明、也不可猜测——它编码了生成时刻，这对单用户桌面应用可以接受（其数据库
/// 本就被该用户可读），此处点明以免有人把它用在需要更多保证的场景。计数器才是唯一性所在：
/// 否则同一纳秒内生成的两个键会相撞，而在一个时钟节拍内重启的进程会重复前缀。随机后缀可
/// 去掉这两条隐患，但库仍是本地的，不值得多一个依赖。
pub fn uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sp_{:x}{:x}", nanos, n)
}

/// 在 `Option<T>` 上把 JSON 缺席与 `null` 解析成**不同**状态：
///   - 缺席     → `None` 外层（`patch.field` 未设 → 不碰该列）
///   - `null`   → `Some(None)`       → 向该列写 NULL
///   - `"foo"`  → `Some(Some("foo"))` → 写 "foo"
///
/// serde 内建的 `Option<T>` 把缺席与 `null` 折叠成一回事，这会毁掉 patch 结构体里
/// 「清空该列」与「保持不动」的区分。本包装器把它还原。
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// 测试——跨 IPC 形状的往返序列化。

#[cfg(test)]
#[path = "domain_test.rs"]
mod tests;
