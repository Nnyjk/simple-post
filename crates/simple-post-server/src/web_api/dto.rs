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
    /// 请求体的二进制形态（base64）。有它时 `body` 缺席，线上发出去的是解码后的字节。
    #[serde(default)]
    pub body_base64: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
    /// HTTPS 出站的**全局默认**（设置面板里改的那份）。
    ///
    /// `Option` 而非直接一个 `TlsSettings`：桌面端那个命令收的是 `Option<TlsSettings>`，
    /// 故 `"tls": null` 在桌面端被收下、在 REST 端被拒 —— 同一份 JSON 两条 transport
    /// 判定不同。取 `Option` 后两侧的判定一致（`null` 与缺席都得到缺省值）。
    #[serde(default)]
    pub tls: Option<simple_post_core::domain::TlsSettings>,
    /// 该端点自己的 HTTPS 出站设置。`null` = 继承全局默认。
    ///
    /// 端点给了设置就**整体**取代全局那份，故它不是逐字段的覆盖。
    #[serde(default)]
    pub tls_override: Option<simple_post_core::domain::TlsSettings>,
    /// 端点 id。有它才装配得到脚本链（collection + 端点自己的两段脚本）。
    /// 缺席 = 临时请求，没有脚本。
    #[serde(default)]
    pub endpoint_id: Option<String>,
    /// 脚本看得到的环境。`id` 决定 `pm.environment.set` 往哪写。
    #[serde(default)]
    pub env: Option<simple_post_core::domain::EnvSnapshot>,
    /// 端点自己的前置脚本，**覆盖**存的那份。缺席 = 用存的那份；空串 = 没有脚本。
    ///
    /// 渲染层送的是界面上此刻生效的那份（草稿优先），故未保存的编辑也会跑到——
    /// 否则「所见」与「所发」会分叉。
    #[serde(default)]
    pub pre_request_script: Option<String>,
    /// 端点自己的测试脚本，覆盖规则同上。
    #[serde(default)]
    pub test_script: Option<String>,
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
/// 项目；body 里只放单个服务的属性。
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

// ----- 动态变量 -----------------------------------------------------------

/// `POST /api/dynamic-variables` 的 body。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariableCreateBody {
    /// 归属项目。定义挂在项目里，故它必填：变量名在项目内唯一。
    pub project_id: String,
    pub name: String,
    /// 客户端自铸的 id，非空白时照单采纳——与 `BaseUrlDefCreateBody::id` 同一理由：
    /// renderer 在这一行持久化之前就拿这个 id 当键。
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 数据源端点（`nodes.id`）。与 `source_inline_json` 二选一。
    #[serde(default)]
    pub source_endpoint_id: Option<String>,
    /// 内联请求（method / url / headers / body / auth 的 JSON 文本）。
    #[serde(default)]
    pub source_inline_json: Option<String>,
    /// 取数用的环境。缺席 = 数据源端点所属项目的第一个可用环境。
    #[serde(default)]
    pub source_environment_id: Option<String>,
    /// 取数用的服务（`base_url_definitions.id`）。缺席 = 数据源接口所属项目的默认服务。
    #[serde(default)]
    pub source_base_url_definition_id: Option<String>,
    /// JSONPath 表达式。`$` 表示整段响应。
    #[serde(default)]
    pub extract_path: Option<String>,
    /// 处理脚本。抽取之后跑，用来做 JSONPath 表达不了的加工。
    #[serde(default)]
    pub transform_script: Option<String>,
    /// 值的序列化形态：`auto` / `json` / `text`。
    #[serde(default)]
    pub value_mode: Option<crate::domain::ValueMode>,
    /// 缓存有效期（秒）。
    #[serde(default)]
    pub ttl_seconds: Option<i64>,
    /// 取数失败时是否允许用已过期的缓存值回退。
    #[serde(default)]
    pub stale_fallback: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

/// `POST /api/dynamic-variables/resolve` 的 body。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariableResolveBody {
    /// 要解析的变量名。请求体里写 `{{name}}` 用的那个。
    pub names: Vec<String>,
    /// 在哪个项目里找这些名字。缺席 = 跨项目找（同名定义出现在多个项目里时按歧义
    /// 处理，不猜）。
    #[serde(default)]
    pub project_id: Option<String>,
    /// 真 = 跳过缓存，强制重新取数。
    #[serde(default)]
    pub force: Option<bool>,
}

/// `POST /api/dynamic-variables/{id}/override` 的 body。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariableOverrideBody {
    /// 覆盖值。`null` 或缺席表示清除覆盖值（下次解析重新取数）。
    #[serde(default)]
    pub value: Option<String>,
    /// 覆盖值的到期时间（unix 毫秒）。缺席且 `forever` 为假时沿用定义的 TTL。
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// 真 = 一直有效，直到手动清除。
    #[serde(default)]
    pub forever: Option<bool>,
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
