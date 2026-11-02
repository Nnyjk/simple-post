//! 对外的两个产物类型：调用方的临时覆盖值，与解析器的输出。
//!
//! `EndpointOverrides` 的字段全是 `Option<T>`，于是「没覆盖」与「覆盖成 None / 空」
//! 是两回事。

use serde::Deserialize;

use crate::mcp::key_value::{coerce_key_values, KeyValueInput};
use simple_post_core::domain::{AuthConfig, BodyMode, HttpMethod, RequestBody};
/// 调用方在发送时给出的覆盖值。每个字段都是 `Option<T>`，于是「没覆盖」与「覆盖成
/// None / 空」是两回事。让渲染进程能做 curl 式的临时改动而不改写已存的端点。
#[derive(Debug, Clone, Default)]
pub struct EndpointOverrides {
    pub url: Option<String>,
    pub method: Option<HttpMethod>,
    pub headers: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub params: Option<Vec<simple_post_core::domain::KeyValue>>,
    /// URL 路径里的占位符与它们各自的值。它们替换进路径本身，不进查询串。
    pub path_params: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub body: Option<RequestBody>,
    /// 该端点读的是 `env.moduleBaseUrls` 里哪个模块的**格子**。
    ///
    /// 模块决定取值：地址住在 `(环境 × 模块)`，两个模块可以指向不同 host，而任何一个
    /// 模块的格子都不借用别的模块的。
    ///
    /// 由调用方传入而不是在这里推导，因为本函数是纯的、没有 DB 句柄，而模块住在
    /// `nodes` 表里。`None` 表示「没有树上下文」：没有格子可读，只剩每端点的
    /// `base_url_override` 能产出 base URL。
    pub module_id: Option<String>,
}

/// `overrides` 的线上形状。
///
/// 三个键值对数组走 [`KeyValueInput`]，其余字段与 [`EndpointOverrides`] 同名同形。
/// 单独写一份是这个类型自己写 `Deserialize` 的原因：域类型 `KeyValue` 要求 `id` 与
/// `enabled` 都在，而工具描述与 JSON Schema 文档化的是「只要 `key` 与 `value`」的
/// 那种形状。按域类型直接反序列化，`{"key":"id","value":"42"}` 会因缺字段而失败——
/// 工具描述说能传，调用却报错。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverridesWire {
    url: Option<String>,
    method: Option<HttpMethod>,
    headers: Option<Vec<KeyValueInput>>,
    params: Option<Vec<KeyValueInput>>,
    path_params: Option<Vec<KeyValueInput>>,
    body: Option<RequestBody>,
    module_id: Option<String>,
}

impl<'de> Deserialize<'de> for EndpointOverrides {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = OverridesWire::deserialize(deserializer)?;
        Ok(EndpointOverrides {
            url: wire.url,
            method: wire.method,
            headers: wire.headers.map(coerce_key_values),
            params: wire.params.map(coerce_key_values),
            path_params: wire.path_params.map(coerce_key_values),
            body: wire.body,
            module_id: wire.module_id,
        })
    }
}

/// `resolve_endpoint` 的产物：`{{var}}` 已全部替换、baseUrl 已拼好、鉴权配置已化到
/// 解析后的形态。HTTP 发送器（`simple_post_core::http_executor`）直接消费它。
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub params: Vec<(String, String)>,
    pub body: Option<String>,
    /// 请求体的**二进制形态**（base64）。
    ///
    /// 有它时 `body` 缺席，线上发出去的是解码后的字节。`binary` 模式与带文件分片的
    /// `multipart` 走这条通道。
    pub body_base64: Option<String>,
    /// **套用过 `overrides` 之后**的请求体模式，不是端点行里存的那个。
    ///
    /// multipart 的装配要靠它：`auto_content_type` 对 `Multipart` 返回 `None`
    /// （boundary 只能现生成），由发送方在 execute 之前补上。而调用方给 `overrides.body`
    /// 就能换掉 mode，所以发送方若回头读库里的 `endpoint.body.mode`，同一个请求里
    /// Content-Type 依据的 mode 与装配依据的 mode 就可能不是同一个。
    pub body_mode: BodyMode,
    pub auth: AuthConfig,
    pub missing_vars: Vec<String>,
    /// URL 路径里没取到值的占位符名（`/users/{id}` 的 `id`）。
    ///
    /// 与 `missing_vars` 是两回事：那边是「变量池里没有这个变量」，这边是
    /// 「端点自己的路径参数没有填值」。发送方按这份清单拦下请求——路径参数
    /// 按 OpenAPI 惯例是必填的，缺值发出去只会换一个含义不明的 404。
    pub missing_path_params: Vec<String>,
}
