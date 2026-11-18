//! 取数器抽象与一次解析的上下文。

use std::future::Future;
use std::pin::Pin;

use crate::db;
use crate::domain::{DynamicError, DynamicVariable};

use super::source::SourceContext;

/// 解析的上下文。一次解析里的所有动态变量共享它。
pub struct ResolveContext {
    pub pool: db::DbPool,
    /// 调用方**此刻选中**的环境。定义没有点名取数环境（继承模式）时用它；`None` = 调用方
    /// 没有环境（MCP 不点名、或那个环境不属于数据源端点所属的项目——后一条由
    /// `dynamic::source_context` 判定）。它不参与「指定的环境」：那条由定义自己决定。
    pub environment_id: Option<String>,
}

/// 数据源取回来的原始响应。
pub struct RawSource {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    /// 响应体是否被传输层按 `max_response_size_kb` 裁过。
    ///
    /// 解析链路不看它（截断的正文照样是值），但**预览**必须如实说出来：用户看到半段 JSON
    /// 却以为是接口的问题，会去改一个本来正确的东西。
    pub truncated: bool,
    /// 响应体的**二进制形态**（base64）：字节不是合法 UTF-8 时才有值。
    ///
    /// 有它说明 `body` 是宽松解码的结果，抽取规则（JSONPath）读不了——预览据此明说
    /// 「这是二进制正文」，而不是把一个空树摆出来。
    pub body_base64: Option<String>,
    /// 响应头。处理脚本看得到它们（`pm.response.headers`）。
    pub headers: Vec<(String, String)>,
    /// 往返耗时（毫秒）。
    pub duration_ms: u64,
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
