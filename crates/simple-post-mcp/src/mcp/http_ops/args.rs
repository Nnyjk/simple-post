//! 两个工具的入参形状 —— `api_run_request` 收一整份临时请求，`api_call_endpoint`
//! 收一个端点 id 加一份覆盖值。

use serde::Deserialize;
use serde_json::Value;

/// `api_run_request` 的入参。
#[derive(Debug, Deserialize)]
pub(super) struct RunRequestArgs {
    pub(super) method: String,
    pub(super) url: String,
    /// 请求头。接受 `{"Foo": "bar"}`、`[["Foo","bar"]]`、
    /// `[{"key": "Foo", "value": "bar", "enabled": false}]` 三种形状。
    #[serde(default)]
    pub(super) headers: Option<Value>,
    /// 查询参数。形状与请求头相同。
    #[serde(default)]
    pub(super) params: Option<Value>,
    /// URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。形状与请求头相同。
    ///
    /// 它们替换进路径本身，不像 `params` 那样进查询串。
    #[serde(default)]
    pub(super) path_params: Option<Value>,
    #[serde(default)]
    pub(super) body: Option<String>,
    /// body 的模式（`none` / `json` / `form` / `raw` / `multipart`）。
    /// 给了 body 而没给模式时算 `raw`。
    #[serde(default)]
    pub(super) body_mode: Option<String>,
    /// 鉴权方案，形状与端点存的那份一致：
    /// `{"kind": "bearer", "bearer": "…"}` / `{"kind": "basic", "basic": {…}}` /
    /// `{"kind": "apikey", "apikey": {…}}` / `{"kind": "oauth2", "oauth2": {…}}`。
    #[serde(default)]
    pub(super) auth: Option<Value>,
    /// 这次请求用哪个环境，以及**它属于哪个项目**。
    ///
    /// 这一项不是可选的：临时请求没有端点行可读，项目归属只能由环境 id 反查，
    /// 故不点名就没有项目 —— 既没有默认环境可回落，也没有（环境 × 模块）那一格
    /// 地址可读。那次请求照发，但不带 base URL、`{{var}}` 也不替换。
    #[serde(default)]
    pub(super) env_id: Option<String>,
    /// 取值时读的是哪个模块的格子。地址住在 (环境 × 模块)，不给就没有格子、没有
    /// base URL。
    #[serde(default)]
    pub(super) module_id: Option<String>,
    #[serde(default)]
    pub(super) settings_override: Option<Value>,
    #[serde(default)]
    pub(super) timeout_ms: Option<u64>,
    #[serde(default)]
    pub(super) follow_redirects: Option<bool>,
    #[serde(default)]
    pub(super) max_response_size_kb: Option<u64>,
}

/// `api_call_endpoint` 的入参。
#[derive(Debug, Deserialize)]
pub(super) struct CallEndpointArgs {
    pub(super) endpoint_id: String,
    /// 这次调用按哪个环境解析。缺席 = 用该项目的默认环境（最早创建的那个）。
    ///
    /// 端点行不再绑定环境，故这里是**逐次**的选择：同一个端点可以在这一轮走预发、
    /// 下一轮走生产，两次各发各的。
    #[serde(default)]
    pub(super) environment_id: Option<String>,
    #[serde(default)]
    pub(super) overrides: Option<Value>,
    #[serde(default)]
    pub(super) settings_override: Option<Value>,
}

/// `api_run_collection` 的入参。
#[derive(Debug, Deserialize)]
pub(super) struct RunCollectionArgs {
    /// 要跑的集合 id。与 `module_id` 二选一。
    #[serde(default)]
    pub(super) collection_id: Option<String>,
    /// 要跑的模块 id（它下面的全部接口）。与 `collection_id` 二选一。
    #[serde(default)]
    pub(super) module_id: Option<String>,
    #[serde(default)]
    pub(super) settings_override: Option<Value>,
}
