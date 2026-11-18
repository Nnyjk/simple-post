//! 资源树：module / collection / endpoint 的节点行，以及 endpoint 的请求配置。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::auth::{AuthConfig, KeyValue, RequestBody};
use super::http::HttpMethod;
use super::settings::TlsSettings;
use super::util::serialises_to_empty_object;

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
    /// SQLite 的 CHECK 约束读不到别的行，无法在多态 `parent_id` 上表达这个矩阵，故矩阵写在这里，
    /// 由 `db::nodes` 每次写入时执行，`schema.sql` 另有配套触发器。
    /// endpoint 的父是 module 或 collection：模块直接收 endpoint，免得「一个 endpoint」这种最简场景
    /// 先得为此建一个用完即弃的 collection。
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
            // collection 用来把 endpoint 与同级分组；模块直接收下不需要独立文件夹的那个。
            ResourceNodeType::Endpoint => matches!(
                parent,
                ResourceNodeType::Module | ResourceNodeType::Collection
            ),
        }
    }

    /// 本类型的节点能否落在项目顶层（`parent_id = NULL`）。只有 module 可以。
    ///
    /// 前端 `src/lib/node-placement.ts` 读的是同一张矩阵的镜像，两侧由
    /// `fixtures/placement-parity.json` 钉住。
    pub fn may_sit_at_project_top(self) -> bool {
        matches!(self, ResourceNodeType::Module)
    }
}

/// 资源树的一行：只有结构与身份。endpoint 的请求配置在 [`EndpointDetail`] 里，按 `node_id` 关联。
///
/// `method` / `url` / `tags` 对模块毫无意义，折叠进来会让每个模块和 collection 行永远带着空列。
///
/// `src/types/domain.ts` 侧的镜像多一个字段：渲染器的折叠/展开态 `expanded?: boolean`。此处刻意不要
/// ——它既不落库也不属于节点身份。本结构体忽略未知键，渲染层带 `expanded` 回传会被直接丢弃；用
/// `#[serde(deny_unknown_fields)]` 堵这个口是错的，它会连该字段一并拒绝，也会破坏 [`EndpointResource`]
/// 的 `#[serde(flatten)]`。
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    /// 资源自己的身份字段：`id` / `rootId` / `kind` / `createdAt` / `updatedAt`。
    ///
    /// 「取回再改再写回」是公开契约：客户端 GET 一个节点，改一处，再把整个 body PATCH
    /// 回去。补丁类型带 `deny_unknown_fields`，故它必须认得资源自己的每一个字段名，
    /// 否则这条往返从一个 400 开始。
    ///
    /// 这几个字段只被**接受**，不被写入：节点的身份由路径里的 id 决定，而创建与更新时间
    /// 由写路径自己盖。它们用 `skip_serializing` 而不是 `skip_serializing_if`：它们不
    /// 指向任何可写列，故不该让 `is_empty` 读起来像「有东西要写」。
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    #[serde(default, skip_serializing)]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing)]
    pub kind: Option<ResourceNodeType>,
    #[serde(default, skip_serializing)]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing)]
    pub updated_at: Option<i64>,
}

impl ResourceNodePatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// 一条响应的**文档定义**：这个端点会回什么。与请求侧的 `params` / `body.schema` 对称。
///
/// 这只是文档：发送请求时不读它，响应历史也不写回它，故它与实际收到的响应可以不一致。
/// 它的去处是导出的 OpenAPI 文档与 agent 的入参。
///
/// 字段形状由 OpenAPI 的 response 对象决定：`description` 在那份规范里必填，故这里也是必填字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDefinition {
    /// 行身份，只给界面列表与 PATCH 定位用。缺省时由调用方铸 id（`spec::input` 的 coercion
    /// 铸 `resp_` 前缀），与 [`KeyValue`] 的 `id` 同一套做法。
    #[serde(default)]
    pub id: String,
    /// `200` / `2XX` / `default` 三种写法之一，判定见 [`is_response_status`]。
    #[serde(default)]
    pub status: String,
    /// 这个响应是什么。OpenAPI 只要求它存在，空串导出成 `""`。
    #[serde(default)]
    pub description: String,
    /// 例 `application/json`。空串表示导出时不写 `content` 块，只留 `description`。
    #[serde(default)]
    pub content_type: String,
    /// 响应体的 JSON Schema（对象）。`None` = 没有 schema 可写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
    /// 示例正文的原文。导出时能解析成 JSON 就写成 JSON 值，否则按字符串写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// 响应定义里查出的一个字段级问题。`index` 指向它在列表中的位置。
///
/// 存储层的写入口（`db::endpoint_details::{create,update}`）与 MCP 的 `api_validate_endpoint`
/// 都读这个结构：前者把问题折成一条 `Validation` 错误，后者逐条报成 issue。**判据只在这里
/// 实现一次**，两侧各自只负责措辞：`Validation` 走中文（与 core 其余校验错误一致），MCP 的
/// issue 走英文（与 `api_validate_endpoint` 其余 issue 一致），故 `kind` 同时带着「哪条规则
/// 被踩了」，措辞按 `kind` 分派。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseProblem {
    pub index: usize,
    /// 结构体字段名：`status` / `schema` / `contentType`。
    pub field: &'static str,
    pub kind: ResponseProblemKind,
    pub message: String,
}

/// 响应定义踩的是哪一条规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseProblemKind {
    /// `status` 不是 `200` / `2XX` / `default` 之一。
    StatusSyntax,
    /// 同一份定义里 `status` 出现了第二次。
    StatusDuplicate,
    /// `schema` 给了，但不是 JSON 对象。
    SchemaNotObject,
    /// `contentType` 非空，但不是 `type/subtype` 形状。
    ContentType,
}

impl ResponseProblem {
    /// 线上字段路径：`responses[0].status`。MCP issue 的 `field` 用的就是这个写法。
    pub fn field_path(&self) -> String {
        format!("responses[{}].{}", self.index, self.field)
    }
}

/// 给缺 id 的响应定义铸一个 `resp_` 前缀的 id，其余原样返回。
///
/// 三条写路径都在入口调它：`spec::input` 的 coercion（MCP 入参与 spec upsert）、
/// `db::endpoint_details` 的写入（REST 直达的那条）、`interchange::commit` 的落库（导入的
/// `IrEndpoint` 可能不带 id）。界面靠这个 id 定位行，故任何一条路径都不能漏。
pub fn ensure_response_ids(mut responses: Vec<ResponseDefinition>) -> Vec<ResponseDefinition> {
    for response in &mut responses {
        if response.id.trim().is_empty() {
            response.id = super::util::uid_with_prefix("resp");
        }
    }
    responses
}

/// 校验一整份响应定义：逐条字段规则，外加「同一份里 `status` 不得重复」。
///
/// 规则只在这里实现一次，故 REST 与 MCP 两条写路径无法各说各话。
pub fn response_problems(responses: &[ResponseDefinition]) -> Vec<ResponseProblem> {
    let mut problems = Vec::new();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (index, response) in responses.iter().enumerate() {
        if !is_response_status(&response.status) {
            problems.push(ResponseProblem {
                index,
                field: "status",
                kind: ResponseProblemKind::StatusSyntax,
                message: format!(
                    "status `{}` 不是合法值：只接受 1XX–5XX 的三位数字、同范围的 `NXX`（如 `2XX`）与字面 `default`",
                    response.status
                ),
            });
        } else if let Some(first) = seen.insert(response.status.as_str(), index) {
            problems.push(ResponseProblem {
                index,
                field: "status",
                kind: ResponseProblemKind::StatusDuplicate,
                message: format!("status `{}` 与第 {} 条重复", response.status, first),
            });
        }
        if let Some(schema) = &response.schema {
            if !schema.is_object() {
                problems.push(ResponseProblem {
                    index,
                    field: "schema",
                    kind: ResponseProblemKind::SchemaNotObject,
                    message: "schema 必须是 JSON 对象".to_string(),
                });
            }
        }
        let content_type = response.content_type.trim();
        if !content_type.is_empty() && !is_media_type(content_type) {
            problems.push(ResponseProblem {
                index,
                field: "contentType",
                kind: ResponseProblemKind::ContentType,
                message: format!(
                    "contentType `{content_type}` 不是合法的 media type：要写成 `type/subtype`"
                ),
            });
        }
    }
    problems
}

/// `status` 的合法写法，全小写的 `default` 是唯一例外：`200`、`2XX`（大写字母）或 `default`。
///
/// 三位数字的首位限定在 1–5，与 OpenAPI 的响应状态码取值范围一致。小写的 `2xx` 不接受：
/// 同一种写法在文档里只应有一种拼法。
pub fn is_response_status(status: &str) -> bool {
    if status == "default" {
        return true;
    }
    match status.as_bytes() {
        // 守卫必须写在模式里：写在分支体里时 `[first, rest, last]` 已经匹配上了三字节的形状，
        // 后面那条 `2XX` 分支永远轮不到，`2XX` 就被判成非法。
        [first, rest, last] if rest.is_ascii_digit() && last.is_ascii_digit() => {
            (b'1'..=b'5').contains(first)
        }
        [first, b'X', b'X'] => (b'1'..=b'5').contains(first),
        _ => false,
    }
}

/// `type/subtype` 的够用判定，不引解析库。
///
/// 参数（`; charset=utf-8`）按 media type 的规定忽略；两个 token 都必须非空且只由 RFC 7230 的
/// token 字符组成，中间恰好一个 `/`。通配符 `*` 属于 token 字符，故 `application/*` 也通过——
/// OpenAPI 的 `content` 键允许 media range。
///
/// 公开是因为导出的 OpenAPI 要拿同一个判据决定「这个 `content` 键能不能写」：写进去的键
/// 过不了规范校验，整份文档都会被判为非法。
pub fn is_media_type(raw: &str) -> bool {
    let essence = raw.split(';').next().unwrap_or("").trim();
    let mut parts = essence.split('/');
    let (Some(kind), Some(subtype), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && kind.bytes().all(is_token_byte)
        && subtype.bytes().all(is_token_byte)
}

/// RFC 7230 的 tchar：字母、数字与 `!#$%&'*+-.^_`|~`。
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
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
}

/// 一个端点用哪种传输发出去。`http` 是默认值，其余三种是 issue #12 的协议扩展。
///
/// 线上值全小写：`http`、`graphql`、`sse`、`websocket`。存储层把这一列存成文本，
/// 读回时按文本解析，未知值报错而不是退回默认值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestProtocol {
    #[default]
    Http,
    GraphQl,
    Sse,
    WebSocket,
}

impl RequestProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequestProtocol::Http => "http",
            RequestProtocol::GraphQl => "graphql",
            RequestProtocol::Sse => "sse",
            RequestProtocol::WebSocket => "websocket",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "http" => Some(RequestProtocol::Http),
            "graphql" => Some(RequestProtocol::GraphQl),
            "sse" => Some(RequestProtocol::Sse),
            "websocket" => Some(RequestProtocol::WebSocket),
            _ => None,
        }
    }
}

/// GraphQL 端点的请求定义。`protocol == graphql` 时 `resolve_endpoint` 用它组装请求体；
/// 协议是别的值时这一份不参与组装。
///
/// `variables` 存原始文本而不是 `Value`：界面里是一个可编辑的文本框，用户在写的过程中
/// 它不是合法 JSON，存成 `Value` 会在输入途中就失败。合法性在发送与校验时才判定。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub variables: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
}

/// endpoint 的请求配置与响应定义。按 `node_id` 与 [`ResourceNode`] 关联；树的列不出现在这里，
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
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    /// URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
    ///
    /// 与 `params` 分开存，因为两者进的地方不同：`params` 一律进查询串，路径参数要被替换进
    /// 路径本身。混在一份列表里，改一个占位符的名字会静默改变一个参数的归属。
    #[serde(default)]
    pub path_params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub auth: AuthConfig,
    /// 这个端点会回什么。文档定义，不上线：发送请求不读它，响应历史也不写它。
    ///
    /// `#[serde(default)]` 与 `responses_json` 列的 `DEFAULT '[]'` 是同一件事的两处表达：
    /// 老库补列之后，既有端点的这里读出空列表。
    #[serde(default)]
    pub responses: Vec<ResponseDefinition>,
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
    /// 该端点自己的 HTTPS 出站设置。`None` = 继承全局默认（`RequestSettings::tls`）。
    ///
    /// 端点给了设置就**整体**取代全局那份，故这一个字段足以表达「这个接口用另一套证书」。
    /// 合成规则见 [`crate::tls::resolve`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsSettings>,
    /// 这个端点的传输协议。缺省的 `http` 是绝大多数端点的值，也是老库补列后的读回值。
    #[serde(default)]
    pub protocol: RequestProtocol,
    /// GraphQL 的 query / variables。`None` = 没有写过；`protocol == graphql` 时
    /// `resolve_endpoint` 按空 query 组装 —— 空 query 由 `api_validate_endpoint` 报出。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphQlRequest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointDetailPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<KeyValue>>,
    /// 缺席 = 不动，`null` = 清空，设值 = 写入。
    ///
    /// 走 `double_option`：内建的 `Option<Vec<KeyValue>>` 把「键缺席」与「值是 `null`」
    /// 折叠成同一件事，于是 `{"pathParams": null}` 清不掉任何东西。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_params: Option<Option<Vec<KeyValue>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<KeyValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    /// 缺席 = 不动，`null` = 清空，设值 = 整体替换。
    ///
    /// 走 `double_option`，理由同 `pathParams`：`Option<Vec<_>>` 会把「键缺席」与「值是 `null`」
    /// 折叠成同一件事，`{"responses": null}` 就清不掉任何定义。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<Option<Vec<ResponseDefinition>>>,
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
    /// 缺席 = 不动，`null` = 清除（回到继承全局），设值 = 写入。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<Option<TlsSettings>>,
    /// 缺席 = 不动，设值 = 换一种传输。没有 `null` 形态：协议总有一个值，缺省是 `http`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<RequestProtocol>,
    /// 缺席 = 不动，`null` = 清空，设值 = 整体替换。
    ///
    /// 走 `double_option`：`Option<GraphQlRequest>` 会把「键缺席」与「值是 `null`」
    /// 折叠成同一件事，于是 `{"graphql": null}` 清不掉已存的 query。
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graphql: Option<Option<GraphQlRequest>>,
    /// 这份配置属于哪个节点。客户端 GET 一份配置再 PATCH 回去时会把它一起带回来，故
    /// 补丁类型认得它——但它只被**接受**，不被写入：节点的 id 由路径决定。
    #[serde(default, skip_serializing)]
    pub node_id: Option<String>,
}

impl EndpointDetailPatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// 树与编辑器共同看到的 endpoint：节点结构 + 请求配置。
/// 与 [`ResourceNode`] 分开，好让树的读取可以完全跳过 `endpoint_details`——树只渲染
/// name / type / sort_order。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointResource {
    #[serde(flatten)]
    pub node: ResourceNode,
    pub detail: EndpointDetail,
}
