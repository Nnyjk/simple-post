//! `endpoint_*` 工具的入参形状，以及端点 id 的铸造。
//!
//! 这一层只反序列化，不碰 DB：真正的校验与强制转换在 `input.rs`，落库在 `write.rs`。

use serde::Deserialize;
use serde_json::Value;

use crate::mcp::key_value::KeyValueInput;
use simple_post_core::domain::TlsSettings;

/// `endpoint_create` 的入参。
///
/// `deny_unknown_fields`：schema 只声明 `additionalProperties: true` 时，serde 会静默丢掉
/// 后端不读的字段。不认识的字段改为报错，调用方才能发现字段名拼错。
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
    /// URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
    ///
    /// 它们替换进路径本身，不像 `params` 那样进查询串；占位符的名字要与 `url` 里
    /// 那一段对上。
    #[serde(default)]
    pub path_params: Option<Vec<KeyValueInput>>,
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
    /// 该端点自己的 base URL。`None` = 读环境里那个模块的格子。
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
    /// 该端点自己的 HTTPS 出站设置。`None` = 继承全局默认。
    ///
    /// 给了设置就**整体**取代全局那份，不做逐字段合并。证书只存文件路径，路径由
    /// 运行请求的那台机器解析。
    #[serde(default)]
    pub tls: Option<TlsSettingsInput>,
}

/// `tls` 载荷的线格式。
///
/// **与 `simple_post_core::domain::TlsSettings` 分开是刻意的。** 域类型要负责读回库里
/// 存的那一行，给它加 `deny_unknown_fields` 会让存着别的键名的旧行读不出来。而线上给
/// agent 的载荷**必须**把拼错的键名报出来：静默丢掉一个键会写进一份「看起来设好了、
/// 其实没有」的设置 —— 而端点自带的那份整体取代全局默认，于是全局的「跳过校验」与
/// CA 路径在这个端点上全部失效，请求报「certificate signed by unknown authority」，
/// 调用方却以为自己已经打开了跳过校验。
///
/// 键名是 camelCase，与 `TlsSettings` 的 serde 键名一致。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct TlsSettingsInput {
    /// 跳过服务端证书校验，主机名一并跳过。
    #[serde(default)]
    pub skip_verify: bool,
    /// 自定义 CA 证书的 PEM 文件路径。
    #[serde(default)]
    pub ca_certificate_path: Option<String>,
    /// 客户端证书的 PEM 文件路径（mTLS）。
    #[serde(default)]
    pub client_certificate_path: Option<String>,
    /// 客户端私钥的 PEM 文件路径。
    #[serde(default)]
    pub client_key_path: Option<String>,
}

impl From<TlsSettingsInput> for TlsSettings {
    fn from(v: TlsSettingsInput) -> Self {
        TlsSettings {
            skip_verify: v.skip_verify,
            ca_certificate_path: v.ca_certificate_path,
            client_certificate_path: v.client_certificate_path,
            client_key_path: v.client_key_path,
        }
    }
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
