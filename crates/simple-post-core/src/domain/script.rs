//! 脚本的线上类型：端点与 collection 上的两段脚本，以及一次脚本执行的结果。
//!
//! 这些是跨 IPC / MCP 边界的线格式，字段名一律 camelCase，渲染层无需翻译。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::util::serialises_to_empty_object;

/// 一个节点上的两段脚本。
///
/// 端点的脚本住在 `endpoint_details`（与 body / headers 同级），collection 的脚本住在
/// `node_scripts` 表。两者线上形状相同，故用同一个结构体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeScripts {
    pub node_id: String,
    /// 请求发出之前跑的那段。`None` = 没有脚本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<String>,
    /// 响应回来之后跑的那段。`None` = 没有脚本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_script: Option<String>,
}

/// 脚本的 patch：缺席 = 不动，`null` = 清除，设值 = 写入。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeScriptsPatch {
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_request_script: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_script: Option<Option<String>>,
}

impl NodeScriptsPatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// 脚本在哪个阶段跑。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptPhase {
    /// 请求发出之前。
    Pre,
    /// 响应回来之后。
    Test,
}

/// 一条脚本日志。`level` 是 `log` / `info` / `warn` / `error`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLog {
    pub level: String,
    pub message: String,
}

/// 一条命名断言的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    /// 失败时的消息；通过时为 `None`。
    pub error: Option<String>,
}

/// 脚本失败的原因分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptErrorKind {
    /// 语法错误，脚本根本没跑起来。
    Compile,
    /// 运行时抛错。
    Runtime,
    /// 超过墙钟截止，被引擎中断。
    Timeout,
    /// 超过内存上限。
    Memory,
    /// 超过栈上限。
    Stack,
}

impl ScriptErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ScriptErrorKind::Compile => "compile",
            ScriptErrorKind::Runtime => "runtime",
            ScriptErrorKind::Timeout => "timeout",
            ScriptErrorKind::Memory => "memory",
            ScriptErrorKind::Stack => "stack",
        }
    }
}

/// 一个脚本的失败。`source` 指明是哪一段脚本出的问题。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptError {
    pub kind: ScriptErrorKind,
    pub message: String,
    /// 出问题的那段脚本的来源描述（collection 名 / 端点名）。
    pub source: String,
    /// 出问题的是哪个阶段。
    pub phase: ScriptPhase,
}

/// 一条环境变量写入。`value` 为 `None` 表示删除该变量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvWrite {
    pub key: String,
    pub value: Option<String>,
}

/// 脚本执行时看到的环境快照。
///
/// `id` 决定 `pm.environment.set` 往哪写；`None` 表示没有环境，那种写入会被丢弃
/// 并记一条警告——无处可写时静默丢弃比报错更让人困惑。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshot {
    pub id: Option<String>,
    pub variables: BTreeMap<String, String>,
}

/// 一次发送里脚本的全部产出。
///
/// 脚本失败**不影响请求的成败**：错误进 `errors`，请求照发。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptOutcome {
    pub logs: Vec<ScriptLog>,
    pub assertions: Vec<AssertionResult>,
    /// 每个失败脚本一条。
    pub errors: Vec<ScriptError>,
    /// 脚本写下的环境变量（已由宿主落库的那些）。
    pub env_writes: Vec<EnvWrite>,
}

impl ScriptOutcome {
    /// 一个没有产出也没有失败的阶段。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 断言有失败的吗？失败**不**改变请求的成败，只在这里呈现。
    pub fn has_failed_assertions(&self) -> bool {
        self.assertions.iter().any(|a| !a.passed)
    }

    /// 把另一个阶段的产出并进来。一次发送跑两个阶段（前置与测试），结果合成一份。
    pub fn merge(&mut self, other: ScriptOutcome) {
        self.logs.extend(other.logs);
        self.assertions.extend(other.assertions);
        self.errors.extend(other.errors);
        self.env_writes.extend(other.env_writes);
    }
}
