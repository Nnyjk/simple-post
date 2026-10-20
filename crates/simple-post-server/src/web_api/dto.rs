// -------------------------------------------------------------------------
// 线格式 DTO（camelCase 以对齐 TS 侧）+ 线格式辅助函数。
// -------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthDto {
    pub status: &'static str,
    pub version: &'static str,
    pub shape: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSendBody {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub status: String,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl From<crate::dto::McpStatusDto> for McpStatusDto {
    fn from(s: crate::dto::McpStatusDto) -> Self {
        // Tauri 命令 DTO 形状已经对了——逐字段拷贝，任一侧改名都不会静默失同步。
        Self {
            status: s.status,
            running: s.running,
            pid: s.pid,
            port: s.port,
            uptime_ms: s.uptime_ms,
            requests_handled: s.requests_handled,
            current_connections: s.current_connections,
            registered_tools: s.registered_tools,
            last_error: s.last_error,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PreferencesPutBody {
    /// 要设置的单键。直接对应 `preferences` 表的一行。
    pub key: String,
    /// JSON 编码的值。由 `db::preferences::set` 原样存下。
    pub value: Value,
}

// -------------------------------------------------------------------------
// 线格式辅助函数——本身很小，但服务端注入的字段都从这里走，好让 SPA 与 REST 的
// 契约与 Tauri 命令路径保持 1:1。
// -------------------------------------------------------------------------

/// 调用方（SPA）没给 `id` 时补一个服务端生成的。镜像每个 `commands::*_create` 的
/// 做法：取用户可见字段，再盖上 `id = uid()`。不补的话每次 REST create 都会以
/// `[validation] missing field 'id'` 失败——domain 结构体全都要求 `id`，而 SPA 只发
/// name / description / color 等。
pub fn ensure_server_id(body: &mut Value) {
    let needs_fill = match body.get("id") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if needs_fill {
        body["id"] = json!(crate::domain::uid());
    }
}

// -------------------------------------------------------------------------
// 资源 CRUD 面的线格式 DTO。这一块保持精简——已在 `domain::*` 里的形状直接复用，
// 使线格式契约与 TS 侧保持 1:1。
// -------------------------------------------------------------------------

/// `POST /api/environments/:envId/vars` 的 body。`value = null` 表示删除该变量
/// （与 `db::environments::set_var` 的「None 移除键」语义一致）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetVarBody {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// `POST /api/environments/:envId/vars/rename` 的 body。同时带改名的两半，好让后端
/// 在一个事务里做完——客户端绝不能把它拆成「先删后设」，那中间会有变量不存在的窗口。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvRenameVarBody {
    pub from_key: String,
    pub to_key: String,
}

/// `POST /api/environments/:envId/vars/disabled` 的 body。
///
/// 停用的变量照样存、照样显示，但不参与替换。`disabled = false` 是启用。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetVarDisabledBody {
    pub key: String,
    pub disabled: bool,
}

/// `POST /api/environments/:envId/module-baseurls` 的 body。`url = null` 清空这一格。
/// 这是 URL 唯一所在的一层——上面没有别的层，也不向别的模块的值回落。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetModuleBaseUrlBody {
    pub def_id: String,
    /// 正在写入该服务地址的模块。同一个服务可以对两个模块指向两个不同主机，故
    /// `moduleId` 是地址的一部分而非过滤条件。
    pub module_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// `POST /api/environments/active` 的 body。`envId` 放在 body 而非 URL 里，好让字面量
/// 路由 `/active` 能充当「设置当前环境」的动词而不与 `/api/environments/:envId` 冲突。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveEnvBody {
    pub env_id: String,
}

/// `PUT /api/permissions/:projectId` 的 body。`permission` 字符串经
/// [`domain::Permission::parse`] 解析——调用点拼错会以 `AppError::Validation` 失败，
/// 而不是写进一行垃圾数据。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPutBody {
    pub permission: String,
}

/// `POST /api/projects/:projectId/baseurls` 的 body。项目放在路径里，因为服务列表属于
/// 项目；body 里只承载单个服务的属性。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefCreateBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 客户端自铸的 id，非空白时照单采纳——REST 侧的
    /// `commands::dto::CreateBaseUrlDefOpts::id`。两条 transport 出于同一理由收它：
    /// renderer 在这一行持久化之前就拿这个 id 当项目 ★ 与每格（环境 × 模块）URL
    /// 单元格的键，服务端铸的 id 会让所有引用变成孤儿。
    #[serde(default)]
    pub id: Option<String>,
}

/// `POST /api/projects/:projectId/baseurls/default` 的 body。`defId = null` 表示把项目
/// 的 ★ 完全撤掉——新建项目正处于这个状态。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefSetDefaultBody {
    #[serde(default)]
    pub def_id: Option<String>,
}

// ----- 资源树 -----------------------------------------------------------

/// `POST /api/nodes` 的 body。
///
/// endpoint 以一次请求带来两半而非两次：没有配置的节点打不开，而两次调用会留下一个
/// 「树里有个 endpoint、选中却报错」的窗口。`db::nodes::create` 强制这个配对。
#[derive(Debug, Deserialize)]
pub struct NodeCreateBody {
    #[serde(flatten)]
    pub node: NodeCreateNodeBody,
    /// endpoint 才有，其余节点不带。任一侧不匹配都被 `db::nodes` 拒绝（不在这里），
    /// 故这条规则只有一处。
    ///
    /// 用 `CreateEndpointDetailOpts` 而非 `EndpointDetail`：后者每个字段都必填，而从
    /// 树的「新建请求」建出的 endpoint 只带用户目前敲下的内容——Tauri 命令一直接受
    /// 这种形状（见该类型自己的文档注释）。在这里直接反序列化成 `EndpointDetail` 会让
    /// `method`——它没有默认值——只在 REST 这一条 transport 上变成必填，于是同一次点击
    /// 在桌面端建出 endpoint、在 `serve-web` 下回 400。复用命令的类型正是为了防止两条
    /// transport 再次漂移。
    #[serde(default)]
    pub detail: Option<crate::dto::CreateEndpointDetailOpts>,
}

/// `POST /api/nodes` body 中结构性的那一半。
///
/// 不是 `ResourceNode`。后者是*存储*行，`rootId` 恒在（数据库保证）；但 create 调用在
/// 指定父节点时没有理由知道所属项目，而 renderer 会发 `rootId: null` 来表达这层意思。
/// 按存储形状反序列化会让「让服务端自己推」成为这条 transport 上客户端唯一无法表达
/// 的诉求（`400 invalid type: null, expected a string`）；这里的 `root_id:
/// Option<String>` 与 Tauri 命令同名参数是同一份契约。
///
/// `id` 必填，因为解析本类型之前 `ensure_server_id` 已补上它；时间戳缺席，因为由
/// 服务端盖。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCreateNodeBody {
    pub id: String,
    /// `None` / 空表示「由 `parentId` 推导」。
    #[serde(default)]
    pub root_id: Option<String>,
    /// 模块为 `None`，它的父节点是项目。
    #[serde(default)]
    pub parent_id: Option<String>,
    pub kind: crate::domain::ResourceNodeType,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

impl NodeCreateNodeBody {
    /// `db::nodes::create` 将要写入的那一行。`root_id` 由这里传入而非从 body 里读，
    /// 免得调用方传出一个节点与一个对「谁拥有它」各执一词的项目。
    pub(crate) fn into_node(self, root_id: &str) -> crate::domain::ResourceNode {
        let now = crate::domain::now_ms();
        crate::domain::ResourceNode {
            id: self.id,
            root_id: root_id.to_string(),
            parent_id: self.parent_id,
            kind: self.kind,
            name: self.name,
            description: self.description,
            sort_order: self.sort_order.unwrap_or_default(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// `POST /api/nodes/move` 的 body。
///
/// `nodeIds` 就是节点在目标位置应出现的顺序，故多选拖拽能保持用户的选择次序。
///
/// `parentId` 取代了旧的 `parentKind` + `parentId` 组合：目的地位于哪张表，如今是父
/// 节点 `type` 的问题，由 `db::nodes::move_many` 回答。`null` 表示项目顶层，且仅对模块
/// 合法。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMoveBody {
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub index: i32,
}

#[cfg(test)]
#[path = "dto_test.rs"]
mod tests;
