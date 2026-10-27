//! `endpoint_*` 工具的入参形状，以及端点 id 的铸造。
//!
//! 这一层只反序列化，不碰 DB：真正的校验与强制转换在 `input.rs`，落库在 `write.rs`。

use serde::Deserialize;
use serde_json::Value;

use super::input::KeyValueInput;
/// `endpoint_create` 的入参。
///
/// **`deny_unknown_fields` 不是苛刻，是堵一个洞。** 早先这个结构体没有这个属性，
/// 而 schema 又写着 `additionalProperties: true` —— 于是 agent 传了一个后端不读的
/// 字段（`pre_request_script` 就是其中之一），serde 静默忽略，工具返回成功，
/// 那个字段就这么没了，全程没有任何提示。schema 说「可以传」而实现不读，
/// 等于工具描述在撒谎。改成「不认识的字段直接报错」之后，agent 立刻知道
/// 自己拼错了字段名，而不会拿着一个缺了脚本的端点继续往下走。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EndpointCreateArgs {
    /// 父集合。端点现在可以直接挂在模块下，所以它与 `module_id` 二选一即可。
    #[serde(default)]
    pub collection_id: Option<String>,
    /// 父模块，给不需要自己集合的端点用。
    #[serde(default)]
    pub module_id: Option<String>,
    /// 相对模块的集合路径，如 `订单/明细`。给了它就不用给 `collection_id`：
    /// 中间缺的集合会被逐级建出来，末级就是端点的父。
    #[serde(default)]
    pub collection_path: Option<String>,
    /// 同一个父下已经有 method 与 url 都相同的端点时怎么办。
    /// 缺省是 `reuse`：直接返回那个端点，一个字段都不写。
    #[serde(default)]
    pub if_exists: Option<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub params: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub body_mode: Option<String>,
    #[serde(default)]
    pub body_content: Option<String>,
    /// 可选的 body schema 提示，原样存储，不参与发包。
    #[serde(default)]
    pub body_schema: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_config: Option<Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// 要绑定的环境。建时就收下，新建的端点直接指向正确的环境，省掉第二次调用。
    #[serde(default)]
    pub environment_id: Option<String>,
    /// 该端点据以解析的服务（`base_url_definitions` 的一行）。`None` = 沿用项目 ★。
    #[serde(default)]
    pub base_url_definition_id: Option<String>,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// 前置脚本：在变量替换之后、请求发出之前运行，可以改写请求本身。
    #[serde(default)]
    pub pre_request_script: Option<String>,
    /// 测试脚本：响应到达之后运行，可以断言并写环境变量。
    #[serde(default)]
    pub test_script: Option<String>,
}

/// 生成唯一的端点 id。
///
/// 绝不能用 `ep_` + `now_ms()`：那是毫秒时钟，同一毫秒内两次 `endpoint_create` 会
/// 得到同一个主键，第二次直接撞 `nodes.id` 约束。AI 客户端成批循环创建时必然踩到。
///
/// `domain::uid()` 是本 crate 的 id 唯一来源：纳秒 + 进程内计数器，进程内不撞、重启
/// 也不重复前缀。保留可读的 `ep_` 前缀，与 `col_` / `mod_` / `proj_` 一致。
pub(super) fn mint_endpoint_id() -> String {
    let uid = simple_post_core::domain::uid();
    // `uid()` 给出 `sp_<hex>`；只把通用前缀换成端点的，别在这里重新实现时钟 ——
    // crate 里应当只有一处 id 来源。
    uid.replacen("sp_", "ep_", 1)
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointMoveArgs {
    pub endpoint_id: String,
    pub new_collection_id: String,
    #[serde(default)]
    pub new_sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointSearchArgs {
    #[serde(default)]
    pub project_id: Option<String>,
    /// 收窄到模块的整棵子树。由过门与结果收窄消费，不进 SQL —— `nodes::search` 是
    /// 全表匹配 name / url / description，子树过滤只能在它返回之后做。
    #[serde(default)]
    pub module_id: Option<String>,
    /// 收窄到某个集合的直接子节点。
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointGetArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDeleteArgs {
    pub endpoint_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EndpointDuplicateArgs {
    pub endpoint_id: String,
    #[serde(default)]
    pub name: Option<String>,
}
