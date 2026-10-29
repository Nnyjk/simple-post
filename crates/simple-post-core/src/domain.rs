//! 与 `src/types/domain.ts` 1:1 镜像的共享领域类型。
//!
//! 这些结构体是跨 Tauri IPC 边界的线格式，字段名一律由 `#[serde(rename_all = "camelCase")]`
//! 一次导出，React 侧无需翻译。
//!
//! 持久化层（`crate::db`）把复杂嵌套字段（KeyValue[]、RequestBody、AuthConfig）序列化成
//! JSON TEXT 列，往返见 `db/endpoint_details.rs`。
//!
//! **本模块不允许出现逐字段 `#[serde(rename)]`。** Rust 字段名**就是**线名。逐字段 rename 是
//! 字段名的第二处真相，也正是两种语言开始出现分歧的地方：TypeScript 侧 `in` 是保留字，写不出来。
//! 线名确需不同于 Rust 名时，扩展 Rust 名——故用 `in_location` 而非 `in`；加下划线后缀没用，
//! `in_` camelCase 之后又变回 `in`。

// 按职责分文件；本文件只声明子模块并把公开接口重新导出，故 `domain::Project` 这类旧路径不变。
mod auth;
mod dynamic;
mod http;
mod oauth2;
mod project;
mod resource;
mod response;
mod script;
mod settings;
mod util;

pub use auth::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyFile, BodyMode, FormField,
    KeyValue, MultipartPart, RequestBody,
};
pub use dynamic::{
    DynamicCacheEntry, DynamicError, DynamicErrorKind, DynamicResolution, DynamicValueSource,
    DynamicValueStatus, DynamicVariable, DynamicVariablePatch, ValueMode,
};
pub use http::{parse_http_method, HttpMethod};
pub use oauth2::{OAuth2Auth, OAuth2ClientAuth, OAuth2GrantType, OAuth2TokenLocation};
pub use project::{Environment, EnvironmentPatch, Project, ProjectPatch};
pub use resource::{
    BaseUrlDefinition, EndpointDetail, EndpointDetailPatch, EndpointResource, ResourceNode,
    ResourceNodePatch, ResourceNodeType,
};
pub use response::{
    ApiResponse, ResponseHistoryEntry, StoredRequestSnapshot, StoredResponse,
    RESPONSE_HISTORY_KEEP_DEFAULT, RESPONSE_HISTORY_MAX_BODY_BYTES,
};
pub use script::{
    AssertionResult, EnvSnapshot, EnvWrite, NodeScripts, NodeScriptsPatch, ScriptError,
    ScriptErrorKind, ScriptLog, ScriptOutcome, ScriptPhase,
};
pub use settings::{
    is_loopback_host, McpHttpConfig, Permission, RequestSettings, TlsSettings, DEFAULT_MCP_HOST,
    DEFAULT_MCP_PATH,
};
pub use util::{default_project_color, double_option, now_ms, uid};

// 测试——跨 IPC 形状的往返序列化。

#[cfg(test)]
#[path = "domain/wire_test.rs"]
mod wire_tests;

#[cfg(test)]
#[path = "domain/resource_test.rs"]
mod resource_tests;

#[cfg(test)]
#[path = "domain/patch_test.rs"]
mod patch_tests;

#[cfg(test)]
#[path = "domain/body_test.rs"]
mod body_tests;
