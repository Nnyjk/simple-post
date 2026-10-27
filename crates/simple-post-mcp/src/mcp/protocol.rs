//! JSON-RPC 信封、工具描述符与 dispatch 表的一行。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::state::AppState;

/// JSON-RPC 2.0 响应信封。`result` / `error` 恰有一个是 `Some`，另一个为
/// `None`，被 `#[serde(skip_serializing_if)]` 从线上抹掉。
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 成功响应。
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: Some(result),
            error: None,
        }
    }

    /// 传输层错误响应（不是工具层错误）。
    pub fn err(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// `tools/call` 放在 `result` 里的形状，对应 MCP 的 `CallToolResult`
/// （子集：目前只发 text content）。
///
/// `rename_all = "camelCase"` 是硬约束，不是风格：spec 里失败标志叫 `isError`，
/// 原样序列化 Rust 字段名会被合规客户端整个忽略，于是 LLM 把失败的调用读成成功。
/// 本结构是 `tools/call` result 的唯一塑形来源，所以属性就是全部修复手段。
/// 下面 `CallToolContent` 的 `rename_all = "snake_case"` 与此无关 —— 它管的是
/// `type` **标签**，而该标签只有 `text` 一个变体。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<CallToolContent>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_error: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallToolContent {
    Text { text: String },
}

impl CallToolResult {
    /// 成功结果，包一层 JSON 值（序列化成 text）。
    pub fn json(value: impl Serialize) -> Self {
        let text = serde_json::to_string(&value)
            .unwrap_or_else(|e| format!("{{\"__serialize_error\":\"{}\"}}", e));
        Self {
            content: vec![CallToolContent::Text { text }],
            is_error: false,
        }
    }

    /// 成功结果，包一段纯文本。
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// 错误结果。按 spec，`is_error: true` 仍算一次**成功**的工具调用，
    /// 这样 LLM 能读到消息并继续对话。
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: true,
        }
    }
}

/// 已注册工具的静态描述 —— 名称、说明、`arguments` 的 JSON Schema，`tools/list` 返回。
///
/// `rename_all = "camelCase"` 是硬约束：spec 把这个字段写作 `inputSchema`，发出
/// `input_schema` 会让每个合规客户端都认定该工具没有参数，模型只能从描述文字里
/// 猜参数列表。本 crate 从不回读 descriptor（`Deserialize` 未使用），所以只有
/// 断言序列化结果才拦得住 —— 见 `mod_test.rs`。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// 所有工具都走这个签名。`args` 是客户端发来的完整 `arguments`（没有则为
/// `Value::Null`），缺失/类型不对的字段由 handler 自己归一成
/// `AppError::Validation`。
///
/// `'a` 这个 HRTB 把返回 future 的生命周期绑到 `&AppState` 的借用上；去掉它，
/// boxed `dyn Future` 会退回 `'static`，`&AppState` 借用就过不了编译。
pub type ToolFn = for<'a> fn(
    state: &'a AppState,
    args: Value,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<CallToolResult, simple_post_core::error::AppError>>
            + Send
            + 'a,
    >,
>;

/// dispatch 表的一行。手写而不用 `HashMap`，为了让查表无分支且 `tools/list`
/// 的顺序稳定。
pub(crate) struct ToolEntry {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    /// `arguments` 对象的 JSON Schema。存成 `Value` 字面量，`cargo check`
    /// 不需要 `schemars` 往返。
    pub(crate) schema: Value,
    pub(crate) func: ToolFn,
}
