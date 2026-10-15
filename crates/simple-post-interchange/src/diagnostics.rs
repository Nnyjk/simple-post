//! 诊断通道：让「不静默丢弃」成为类型要求，而不是 review 要求。
//!
//! 三个格式实现共用这一个形状。验收标准里的「畸形 / 不支持的字段给出明确提示」只有
//! 在所有实现都往同一个通道写时才可验证——各写各的，「有提示」就退化成「某个地方
//! 好像提过」。

use serde::{Deserialize, Serialize};

/// 外来格式里没能落进标准格式的东西。
///
/// `pointer` 必须让用户能回到源文件的对应位置：OpenAPI 用 JSON Pointer
/// （`/paths/~1users/get/responses`），Postman 用自己的路径（`/item/2/request/body`）。
/// 只说「第 3 个请求的 body 有问题」在真实文件里没法用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    pub pointer: String,
    /// 人读的一句话，说明发生了什么。写结论，不写过程。
    pub message: String,
    /// 没能落库的那个构造本身（如 `multipart/form-data 的 file 字段`）。
    /// `None` 表示整个条目被跳过，而非局部丢弃。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 不影响结果：等价映射，或按默认处理。
    Info,
    /// 有东西被丢弃或降级映射，结果可用但不完整。
    Warn,
    /// 该条目整体无法导入。
    Error,
}

impl Diagnostic {
    pub fn warn(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            pointer: pointer.into(),
            message: message.into(),
            dropped: None,
        }
    }

    pub fn error(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            pointer: pointer.into(),
            message: message.into(),
            dropped: None,
        }
    }

    /// 附上被丢掉的具体构造。这条是「不静默丢弃」的核心——只说「有东西被忽略」
    /// 与没说没有区别。
    pub fn dropping(mut self, dropped: impl Into<String>) -> Self {
        self.dropped = Some(dropped.into());
        self
    }
}
