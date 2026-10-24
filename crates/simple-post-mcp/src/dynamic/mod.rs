//! 动态变量在 MCP crate 这一侧的部分。
//!
//! 引擎（合并规则、序列化、缓存决策、single-flight）住在 core 的
//! `simple_post_core::dynamic`；取数器住在这里，因为它要用
//! `variable_fill::resolve_endpoint` 组装「引用端点」形态的请求，而那个函数在 mcp crate。
//!
//! core 那边只留一个抽象（`SourceFetcher`），两侧 transport 共用同一份取数逻辑。

pub mod ops;
pub mod source;

pub use ops::{CreateOpts, DynamicVariableView};
pub use simple_post_core::dynamic::{
    decide, extract_value, list_names, merge_pool, resolve_names, serialize_value, Decision,
    RawSource, ResolveContext, SourceFetcher, TemplateParts, MAX_VALUE_BYTES,
};
pub use source::EndpointSourceFetcher;

/// 一次解析用的取数器。两种数据源形态都走它。
pub fn fetcher() -> EndpointSourceFetcher {
    EndpointSourceFetcher
}
