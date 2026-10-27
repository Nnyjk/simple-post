//! 取数器抽象与一次解析的上下文。

use std::future::Future;
use std::pin::Pin;

use crate::db;
use crate::domain::{DynamicError, DynamicVariable};

use super::source::SourceContext;

/// 解析的上下文。一次解析里的所有动态变量共享它。
pub struct ResolveContext {
    pub pool: db::DbPool,
}

/// 数据源取回来的原始响应。
pub struct RawSource {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

/// 取数器：把一条定义的数据源变成一次真实的 HTTP 响应。
///
/// 实现住在 mcp crate（那里有 `variable_fill::resolve_endpoint`）。core 定义这个抽象，是为了让
/// 引擎不必依赖上层 crate。
pub trait SourceFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        ctx: &'a ResolveContext,
        source: &'a SourceContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>>;
}
