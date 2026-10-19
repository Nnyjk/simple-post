//! 交换格式：一棵**还没有 id** 的资源树，外加诊断与丢失集。
//!
//! ## 它为什么不是 `domain` 的别名
//!
//! 三条独立理由，详见 `README.md`：
//!
//! 1. **节点还没有 id。** [`super::ir::IrEndpoint`] 处在「预览数量、确认后落库」之前，
//!    [`simple_post_core::domain::EndpointDetail::node_id`] 此刻根本不存在。
//! 2. **诊断无处安放。** `domain` 每个字段都是标称值，没有位置能说「这里原本是个
//!    multipart file 字段，被丢掉了」。
//! 3. **导出要跨四张表。** host 不在 endpoint 上，URL 按 (环境 × 模块 × 服务) 二级索引，
//!    故投影必须先自己选定环境（见 [`Exchange::environment_id`]）。
//!
//! ## `IrEndpoint::path` 是 path-only
//!
//! 刻意**不**复用 `EndpointDetail`：那个结构体带 `node_id`、`environment_id` 与
//! `base_url_definition_id` 三个此刻要么不存在、要么属于标准格式而非外来格式的身份字段。
//! 外来格式给的是整条 URL，host 的部分要么成为服务、要么成为诊断，两件事都在
//! `format → ir` 这一侧发生，所以 `ir` 存路径。

use simple_post_core::domain::{AuthConfig, HttpMethod, KeyValue, RequestBody, ResourceNodeType};

use crate::diagnostics::Diagnostic;

/// 一次导入或导出的全部内容。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exchange {
    pub project_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_description: Option<String>,
    /// 投影时选定的环境 id。`None` 表示「没选出环境」——不是「不关心」，故缺席即被导出方
    /// 当成一个真实答案（该环境没有为这些模块配 URL）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    pub modules: Vec<IrModule>,
    /// `format → ir` 这一侧丢下的东西。空集才说明导入是完整的。
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/// 模块。只能待在项目顶层，故 [`IrModule`] 里没有任何父指针。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrModule {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 本模块在**选定环境**下为各服务配置的 URL。IR 不带 id，故这里按服务**名**索引：
    /// `base_url_definitions` 是项目级的表，同一项目里两个服务不可能同名。
    ///
    /// 该模块没配就是没有 URL——**没有回退层**，不借用别的模块的值，也不落回项目 ★。
    /// 故缺席是一个真实答案，不是「待补」。
    #[serde(default)]
    pub servers: Vec<IrServer>,
    /// 直接挂在模块下的 endpoint。`ResourceNodeType::may_under` 允许这一层，
    /// 好让「一个 endpoint」这种最简形状不必先建一个用完即弃的 collection。
    #[serde(default)]
    pub endpoints: Vec<IrEndpoint>,
    #[serde(default)]
    pub collections: Vec<IrCollection>,
}

/// 一个 (模块 × 环境) 单元格的实际取值。键是服务名而不是 id：IR 是无 id 的中间态，
/// 而服务表由项目拥有，同项目内名字已无歧义。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrServer {
    /// `base_url_definitions.name`。项目内唯一。
    pub service: String,
    pub url: String,
}

/// 分组。递归嵌套，深度无上限——`may_under` 允许 collection 挂 collection，
/// 故渲染侧才是递归组件。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrCollection {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<IrEndpoint>,
    #[serde(default)]
    pub collections: Vec<IrCollection>,
}

/// 一个请求。`method` / `path` 之外的字段与 `EndpointDetail` 同名同义，
/// 好让 `ir → domain` 是一次字段改名加 id 的搬运，而不是一套映射表。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrEndpoint {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub method: HttpMethod,
    /// **只有路径。** host 来自服务或环境，不在这里——这正是 `ir` 不能是 `domain` 别名的
    /// 第三个理由（见模块文档）。
    pub path: String,
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

impl IrEndpoint {
    /// 只带必填字段的构造。参数 / headers / body / auth 全是「没有就是没有」，
    /// 缺省形状必须与 UI 手工新建一个 endpoint 得到的形状一致。
    pub fn new(name: impl Into<String>, method: HttpMethod, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            method,
            path: path.into(),
            ..Default::default()
        }
    }
}

/// `child` 能否挂在 `parent` 类型的节点下。落库前的预检，与
/// [`ResourceNodeType::may_under`] 同一个矩阵，故只有一处真相。
///
/// 错误消息同时点名父子两种类型：IR 的类型系统已经让「module 下挂 module」写不出来，
/// 但落库前仍要报得出**是哪一对**冲突——真正走到这一步的是外来格式把一棵畸形树
/// 翻译过来的情形。
pub fn may_contain(parent: ResourceNodeType, child: ResourceNodeType) -> Result<(), String> {
    if child.may_under(parent) {
        Ok(())
    } else {
        Err(format!(
            "a {} cannot sit under a {}",
            child.as_str(),
            parent.as_str()
        ))
    }
}

impl IrCollection {
    /// 这棵子树里的 endpoint 数量，含所有层级。落库方用它报告「将新增几个接口」，
    /// 故必须与实际写进去的行数一致——递归是唯一算得准的地方。
    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
            + self
                .collections
                .iter()
                .map(IrCollection::endpoint_count)
                .sum::<usize>()
    }
}

impl IrModule {
    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
            + self
                .collections
                .iter()
                .map(IrCollection::endpoint_count)
                .sum::<usize>()
    }
}

impl Exchange {
    pub fn endpoint_count(&self) -> usize {
        self.modules.iter().map(IrModule::endpoint_count).sum()
    }

    /// 按 `severity` 挑诊断。三个格式实现共用同一形状，故筛选也只有一处。
    pub fn diagnostics_of(&self, severity: crate::diagnostics::Severity) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .collect()
    }
}

// 测试——IR 的树形与前置检查。

#[cfg(test)]
#[path = "ir_test.rs"]
mod tests;
