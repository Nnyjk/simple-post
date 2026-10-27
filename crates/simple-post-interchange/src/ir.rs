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
//!
//! ## `key` 是「IR 还没成为节点」的那一半身份
//!
//! 勾选是前端行为，而前端要能把「第几行」换回「第几个 IR 节点」。落库前没有
//! `nodes.id`，故每个 IR 节点带一个 [`IrEndpoint::key`]：导入时是外来格式里的位置指针
//! （与 [`Diagnostic::pointer`] 同一套寻址纪律），导出时是 `nodes.id`。

use std::collections::BTreeSet;

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
    /// 稳定标识：导入时是源格式里的位置指针，导出时是 `nodes.id`。
    #[serde(default)]
    pub key: String,
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
    /// 这个模块上的两段脚本。
    ///
    /// **模块本身不能存脚本**（`node_scripts` 只认 collection），故这一段只在
    /// 「落进已有模块」时用得上：那时 IR 顶层的模块会降级成集合。落到项目顶层时
    /// 它无处可去，落库侧会出一条诊断说明。
    #[serde(default)]
    pub scripts: IrScripts,
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
    /// 同 [`IrModule::key`]：导入时是源格式里的位置指针，导出时是 `nodes.id`。
    #[serde(default)]
    pub key: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<IrEndpoint>,
    #[serde(default)]
    pub collections: Vec<IrCollection>,
    /// 这个集合上的两段脚本。集合能存脚本，故这两个字段在这里是有归宿的。
    #[serde(default)]
    pub scripts: IrScripts,
}

/// 一个节点上的两段脚本。
///
/// 与 `domain::NodeScripts` 同形，但**没有 `node_id`**：IR 里的节点还没有 id。
/// 端点的脚本落进 `endpoint_details`（与 body / headers 同级），集合的脚本落进
/// `node_scripts` 表。
///
/// `None` = 没有脚本。两个字段都是**源码**，不是路径：外来格式给的就是内联的文本。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrScripts {
    /// 请求发出之前跑的那段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<String>,
    /// 响应回来之后跑的那段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_script: Option<String>,
}

impl IrScripts {
    /// 两段都没有。
    pub fn is_empty(&self) -> bool {
        self.pre_request_script.is_none() && self.test_script.is_none()
    }
}

/// 一个请求。`method` / `path` 之外的字段与 `EndpointDetail` 同名同义，
/// 好让 `ir → domain` 是一次字段改名加 id 的搬运，而不是一套映射表。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrEndpoint {
    /// 稳定标识，供前端勾选与落库时回传。导入时是源格式里的位置指针
    /// （`/item/2`、`/paths/~1users/get`），导出时是 `nodes.id`。
    #[serde(default)]
    pub key: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub method: HttpMethod,
    /// **只有路径。** host 来自服务或环境，不在这里——这正是 `ir` 不能是 `domain` 别名的
    /// 第三个理由（见模块文档）。
    pub path: String,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    /// URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
    ///
    /// 与 `params` 分开：`params` 进查询串，路径参数要替换进路径本身。
    #[serde(default)]
    pub path_params: Vec<KeyValue>,
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
    /// 这个接口上的两段脚本。
    #[serde(default)]
    pub scripts: IrScripts,
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

/// 这个模块子树里所有端点的 [`IrEndpoint::key`]。落库方据此判断「这个容器还有没有
/// 要写的东西」——空容器不写，否则树里会留下一排什么都没有的分组。
pub fn subtree_keys(module: &IrModule) -> BTreeSet<String> {
    endpoint_keys_of(&module.endpoints, &module.collections)
}

/// 一组「本层端点 + 下层分组」里的全部端点 key。模块与集合形状相同，只是模块多一个
/// `servers` 字段，故共这一个走查器。
///
/// **显式工作栈而非递归**：IR 的 collection 嵌套深度无上限，递归会让一棵畸形外来
/// 文件靠栈深度把预览或写路径带走。
pub(crate) fn endpoint_keys_of(
    endpoints: &[IrEndpoint],
    collections: &[IrCollection],
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for ep in endpoints {
        out.insert(ep.key.clone());
    }
    let mut stack: Vec<&IrCollection> = collections.iter().rev().collect();
    while let Some(c) = stack.pop() {
        for ep in &c.endpoints {
            out.insert(ep.key.clone());
        }
        stack.extend(c.collections.iter().rev());
    }
    out
}

/// 就地裁剪一个模块。返回它裁剪后是否还有东西——全空的模块该被丢掉而不是留个空壳。
fn retain_module(m: &mut IrModule, selected: &BTreeSet<String>) -> bool {
    m.endpoints.retain(|e| selected.contains(&e.key));
    m.collections.retain_mut(|c| retain_collection(c, selected));
    !m.endpoints.is_empty() || !m.collections.is_empty()
}

fn retain_collection(c: &mut IrCollection, selected: &BTreeSet<String>) -> bool {
    c.endpoints.retain(|e| selected.contains(&e.key));
    c.collections.retain_mut(|k| retain_collection(k, selected));
    !c.endpoints.is_empty() || !c.collections.is_empty()
}

impl Exchange {
    pub fn endpoint_count(&self) -> usize {
        self.modules.iter().map(IrModule::endpoint_count).sum()
    }

    /// 整份交换里所有端点的 key，即「预览默认勾满」的那一份选择集。
    pub fn endpoint_keys(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for m in &self.modules {
            out.extend(subtree_keys(m));
        }
        out
    }

    /// 只留下 `selected` 里的端点，**以及通往它们的全部祖先**。
    ///
    /// 导出侧用：那里没有指纹可去重，勾选就是唯一的过滤。取消勾选的分组连同整棵
    /// 子树一起消失，产物里不会留下一个空分组。
    ///
    /// 与 [`crate::commit`] 的裁剪**刻意**是两份：落库那份还要问指纹、并把「为什么
    /// 少写了几个」记成诊断，这里只需要纯粹的形状裁剪。共用一份就得让不落库的路径
    /// 也背上一个它用不到的账本。
    pub fn retain_selected(&self, selected: &BTreeSet<String>) -> Exchange {
        let mut out = self.clone();
        out.modules.retain_mut(|m| retain_module(m, selected));
        out
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
