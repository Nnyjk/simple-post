//! 对外的两个产物类型：调用方的临时覆盖值，与解析器的输出。
//!
//! `EndpointOverrides` 的字段全是 `Option<T>`，于是「没覆盖」与「覆盖成 None / 空」
//! 是两回事。

use serde::Deserialize;

use simple_post_core::domain::{AuthConfig, BodyMode, HttpMethod, RequestBody};
/// 调用方在发送时给出的覆盖值。每个字段都是 `Option<T>`，于是「没覆盖」与「覆盖成
/// None / 空」是两回事。让渲染进程能做 curl 式的临时改动而不改写已存的端点。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOverrides {
    pub url: Option<String>,
    pub method: Option<HttpMethod>,
    pub headers: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub params: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub body: Option<RequestBody>,
    /// 覆盖端点上已存的 base URL 定义，改用这个去解析。
    ///
    /// 渲染进程的 base URL 选择器是临时的：用户不保存端点也能切换定义，所以发送时
    /// `detail.base_url_definition_id` 并不权威。没有这个字段，浏览器就得自己先拼
    /// base URL，「URL 组装搬到服务端」这件事就白做了。
    ///
    /// `None` = 用端点自己的值；`Some(id)` = 覆盖；空串 = 「没有 base URL」，
    /// 渲染进程用它表示「对着一条裸路径发请求」。
    pub base_url_definition_id: Option<String>,
    /// 该端点读的是 `env.moduleBaseUrls` 里哪个模块的**格子**。
    ///
    /// 服务属于项目而非模块，所以模块不是「哪些服务存在」的过滤器 —— 每个模块都看到
    /// 全量列表。它选的是取值：URL 住在 `(环境 × 服务 × 模块)`，两个模块可以让同一个
    /// 服务指向不同 host。
    ///
    /// 由调用方传入而不是在这里推导，因为本函数是纯的、没有 DB 句柄，而模块住在
    /// `nodes` 表里。`None` 表示「没有树上下文」：没有格子可读，只剩每端点的
    /// `base_url_override` 能产出 base URL。
    pub module_id: Option<String>,
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
    /// **套用过 `overrides` 之后**的请求体模式，不是端点行里存的那个。
    ///
    /// multipart 的装配要靠它：`auto_content_type` 对 `Multipart` 返回 `None`
    /// （boundary 只能现生成），由发送方在 execute 之前补上。而调用方给 `overrides.body`
    /// 就能换掉 mode，所以发送方若回头读库里的 `endpoint.body.mode`，同一个请求里
    /// Content-Type 依据的 mode 与装配依据的 mode 就可能不是同一个。
    pub body_mode: BodyMode,
    pub auth: AuthConfig,
    pub missing_vars: Vec<String>,
}
