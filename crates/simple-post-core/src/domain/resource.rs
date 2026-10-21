//! 资源树：module / collection / endpoint 的节点行，以及 endpoint 的请求配置。

use serde::{Deserialize, Serialize};

use super::auth::{AuthConfig, KeyValue, RequestBody};
use super::http::HttpMethod;
use super::util::serialises_to_empty_object;

// base url

/// 项目服务表里的一条具名服务。
///
/// 服务表是**项目级**的：每个模块都能看到全部服务。故本结构体没有模块字段，也没有逐行
/// 默认标记——于是一个项目里两个服务不可能同名，endpoint 的 `baseUrlDefinitionId` 在整棵
/// 树上无歧义。表中那两个为已下线的「模块持有」模型服务的列（`module_id` / `is_default`）
/// 仍在表里但已被中和：删列 = 重建表 = bump `SCHEMA_VERSION` = 删掉用户的库。
///
/// 本行只指认服务，从不持有 URL。(服务 × 模块 × 环境) 的 URL 在
/// [`super::Environment::module_base_urls`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefinition {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

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
    /// 关键字，把它在线上叫 `type` 会让 Tauri command 参数与 REST body 各用一套名字。
    pub kind: ResourceNodeType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i64,
    #[serde(default = "super::now_ms")]
    pub created_at: i64,
    #[serde(default = "super::now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNodePatch {
    /// 改父。`None` 保持原父不动；显式 `Some(None)` 把节点提到项目顶层，这只对模块合法。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
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
    /// 请求发出之前跑的那段脚本。`None` = 没有脚本。
    ///
    /// 住在 `endpoint_details` 而不是 `nodes`：脚本对模块与 collection 没有意义，
    /// 而这一行只对 endpoint 存在。collection 的脚本在 `node_scripts` 表。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<String>,
    /// 响应回来之后跑的那段脚本。`None` = 没有脚本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_script: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetailPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
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
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// 缺席 = 不动，`null` = 清除，设值 = 写入。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<Option<String>>,
    /// 缺席 = 不动，`null` = 清除，设值 = 写入。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_script: Option<Option<String>>,
}

impl EndpointDetailPatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
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
