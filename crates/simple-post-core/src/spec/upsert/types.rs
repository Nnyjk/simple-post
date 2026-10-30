//! 规格的线格式：一份规格长什么样，落库之后回什么。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::{AuthConfig, EndpointResource, RequestBody};
use crate::error::AppError;
use crate::spec::input::KeyValueInput;

/// 一份接口规格。
///
/// 字段一律用 `Option` 表达「这份规格没有提到它」，因为「没提到」与「提到但为空」
/// 在落库时的处置不同：前者不动已有的东西，后者要覆盖。
///
/// ## 两种拼写都收
///
/// 字段名是 snake_case（MCP 那条线的惯例），但**camelCase 的别名同样收**：
/// REST 路由与桌面 command 所在的这条线通篇用 camelCase，让调用方为了一份规格
/// 改掉自己的书写习惯没有必要。两种拼写落在**同一个**类型上，故三端不会各收各的形状。
///
/// 别名只挂在多词的字段上；单词字段两种拼写本就相同。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointSpec {
    /// 规格落在哪个项目里。项目必须**已经存在** —— 本模块不建项目，也不改权限档位。
    #[serde(alias = "projectId")]
    pub project_id: String,
    /// 模块名。不存在就建出来。
    #[serde(default)]
    pub module: Option<String>,
    /// 已有的模块 id。与 `module` 二选一。
    #[serde(default, alias = "moduleId")]
    pub module_id: Option<String>,
    /// 集合路径，相对模块，段之间一个斜杠，如 `订单/明细`。路径上缺的集合会被逐级建出来。
    #[serde(default, alias = "collectionPath")]
    pub collection_path: Option<String>,
    /// 已有的集合 id。与 `collection_path` 二选一。
    #[serde(default, alias = "collectionId")]
    pub collection_id: Option<String>,
    /// 环境这一段：环境本身、它的变量、它对某个模块在这个环境下的地址。
    #[serde(default)]
    pub environment: Option<EnvironmentSpec>,
    /// 端点本身。
    pub endpoint: EndpointDefinition,
    /// 同一个父下已经有同名同 method 同 url 的端点时怎么办。缺省是 `reuse`。
    #[serde(default, alias = "ifExists")]
    pub if_exists: Option<String>,
}

/// 规格里的环境那一段。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSpec {
    /// 已有的环境 id。与 `name` 二选一。
    #[serde(default)]
    pub id: Option<String>,
    /// 环境名。不存在就建出来。与 `id` 二选一。
    #[serde(default)]
    pub name: Option<String>,
    /// 要写入的变量。逐个合并进环境已有的变量表 —— 规格是往环境里**加**东西，
    /// 不是把环境重写成规格里那几个变量。
    #[serde(default)]
    pub variables: Option<BTreeMap<String, String>>,
    /// 要停用的变量名。合并进已有的停用集合。
    #[serde(default, alias = "disabledVariables")]
    pub disabled_variables: Option<Vec<String>>,
    /// 这个环境 × 模块 那一格的地址（后端 api 地址）。地址住在那一格，故它随模块走。
    #[serde(default, alias = "baseUrl")]
    pub base_url: Option<String>,
    /// 那一格的实际访问地址——接口经代理网关转代后真正要打的那个地址。缺省 = 没有代理。
    #[serde(default, alias = "baseUrlProxy")]
    pub base_url_proxy: Option<String>,
}

/// 规格里的端点那一段。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDefinition {
    /// 接口名。
    pub name: String,
    /// HTTP 动词。
    pub method: String,
    /// 只有路径。host 在发送时从环境里 (环境 × 模块) 那一格的地址解析。
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub params: Option<Vec<KeyValueInput>>,
    /// 路径参数（`/api/orders/{id}` 的 `id`）与它们各自的值。
    ///
    /// 与 `params` 分开存，因为两者进的地方不同：`params` 一律进查询串，路径参数要被
    /// 替换进路径本身。
    #[serde(default, alias = "pathParams")]
    pub path_params: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub headers: Option<Vec<KeyValueInput>>,
    /// 请求体。`content` 是这一次发出去的载荷，`schema` 是文档里那份参数定义。
    #[serde(default)]
    pub body: Option<RequestBody>,
    /// 认证方案与凭据。`kind` 是方案，缺省 `none`。
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    /// 这个端点自己的 base URL，胜过环境里那一格。
    #[serde(default, alias = "baseUrlOverride")]
    pub base_url_override: Option<String>,
    /// 前置脚本。
    #[serde(default, alias = "preRequestScript")]
    pub pre_request_script: Option<String>,
    /// 测试脚本。
    #[serde(default, alias = "testScript")]
    pub test_script: Option<String>,
}

// =========================================================================
// 端点的冲突策略
// =========================================================================

/// 同一个父下已经有同名同 method 同 url 的端点时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointIfExists {
    /// 复用已有的那一个，**一个字段都不写**。缺省值。
    Reuse,
    /// 报冲突，不新建。
    Error,
    /// 另建一个，与已有那个并存。
    Duplicate,
    /// 把规格里点名的字段覆盖到那个端点上；规格没点名的字段保持原样。
    Update,
}

impl EndpointIfExists {
    /// 从调用方给的名字解析。空串与缺省都当 [`EndpointIfExists::Reuse`]。
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s.trim() {
            "" | "reuse" => Ok(EndpointIfExists::Reuse),
            "error" => Ok(EndpointIfExists::Error),
            "duplicate" => Ok(EndpointIfExists::Duplicate),
            "update" => Ok(EndpointIfExists::Update),
            other => Err(AppError::Validation(format!(
                "未知的 if_exists 值 '{other}'；可取 reuse / error / duplicate / update"
            ))),
        }
    }

    /// 该策略的字符串形式，与 [`EndpointIfExists::parse`] 的取值一一对应。
    pub fn as_str(self) -> &'static str {
        match self {
            EndpointIfExists::Reuse => "reuse",
            EndpointIfExists::Error => "error",
            EndpointIfExists::Duplicate => "duplicate",
            EndpointIfExists::Update => "update",
        }
    }
}

// =========================================================================
// 结果
// =========================================================================
//
// 结果类型本身带 `Serialize` 且字段名是线上形状，故 REST 路由、桌面 command 与
// MCP 工具回的是同一份 JSON，而不必各写一遍拼装 —— 那三份拼装迟早会各写错一遍。

/// 一次落库的结果。
///
/// 三个宿主（REST 路由、桌面 command、MCP 工具）回的是同一份形状：结果类型本身
/// 可序列化，故不必各写一遍 JSON 拼装，也就不会各写错一遍。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecOutcome {
    pub project_id: String,
    /// 端点所在的那个模块。
    pub module: NamedResolved,
    /// 端点的父。模块本身也是一个合法的父。
    pub parent_id: String,
    /// 集合路径上逐级的结果，顺序与路径一致。
    pub collections: Vec<NamedResolved>,
    /// 环境那一段的结果。规格没写环境时为 `None`。
    pub environment: Option<EnvironmentOutcome>,
    /// 端点的结果。
    pub endpoint: EndpointOutcome,
}

/// 一个按名字找到或建出来的实体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedResolved {
    pub name: String,
    pub id: String,
    /// 本次调用**新建**的为 `true`，复用了已有项的为 `false`。
    pub created: bool,
}

/// 环境那一段的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentOutcome {
    pub id: String,
    pub name: String,
    pub created: bool,
}

/// 端点的结果。
///
/// 端点的节点结构与请求配置直接写在这一层：`resource` 带 `#[serde(flatten)]`，
/// 序列化时并进 `endpoint`，故 `endpoint.id` 就是端点的 id，`endpoint.detail` 是它的
/// 请求配置。若把它们收在 `endpoint.resource` 下，调用方就要多挖两级，而那两级里没有信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOutcome {
    /// 本次调用**新建**了这个端点。
    pub created: bool,
    /// 本次调用覆盖了已有端点的字段。
    pub updated: bool,
    /// 端点本身：节点结构 + 请求配置。
    #[serde(flatten)]
    pub resource: EndpointResource,
}
