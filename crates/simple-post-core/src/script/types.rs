//! 沙箱两侧的数据形状：进（请求 / 响应 / 环境）、出（改动 / 写入 / 日志 / 断言）。
//!
//! 这些类型是**宿主与脚本之间唯一的契约**：宿主把它们序列化成 JSON 交给沙箱，沙箱把
//! 改动后的同一批 JSON 交回来。脚本本身拿不到数据库句柄、拿不到网络、拿不到宿主函数。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 一份待发请求的线上形状。脚本看到它，也能改它。
///
/// header 用**有序对列表**而非映射：HTTP 允许重名 header，映射会静默把它们合并，
/// 而线上形态要能原样带到后端（那里收 `Vec<(String, String)>`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// 一份已收到的响应。test 脚本看它。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code: u16,
    pub status: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 往返耗时（毫秒）。
    pub response_time_ms: u64,
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

/// 一条环境变量写入。`value` 为 `None` 表示删除该变量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvWrite {
    pub key: String,
    pub value: Option<String>,
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

/// 一段脚本的来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ScriptSource {
    Collection { id: String, name: String },
    Endpoint { id: String, name: String },
}

impl ScriptSource {
    /// 出错时给人看的名字。名字为空（新建未命名节点）时退回 id。
    pub fn label(&self) -> &str {
        let (id, name) = match self {
            ScriptSource::Collection { id, name } => (id, name),
            ScriptSource::Endpoint { id, name } => (id, name),
        };
        if name.is_empty() {
            id
        } else {
            name
        }
    }
}

/// 链上的一段脚本。`pre_request` / `test` 各自可缺席。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUnit {
    pub source: ScriptSource,
    pub pre_request: Option<String>,
    pub test: Option<String>,
}

/// 一个阶段的执行结果。
///
/// 脚本失败**不影响请求的成败**：错误进 `errors`，请求照发。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptOutcome {
    pub phase: ScriptPhase,
    pub logs: Vec<ScriptLog>,
    pub assertions: Vec<AssertionResult>,
    /// 每个失败脚本一条。
    pub errors: Vec<ScriptError>,
    /// 脚本写下的环境变量（已由宿主落库的那些）。
    pub env_writes: Vec<EnvWrite>,
}

impl ScriptOutcome {
    /// 一个没有产出也没有失败的阶段。
    pub fn empty(phase: ScriptPhase) -> Self {
        Self {
            phase,
            logs: Vec::new(),
            assertions: Vec::new(),
            errors: Vec::new(),
            env_writes: Vec::new(),
        }
    }

    /// 断言有失败的吗？失败**不**改变请求的成败，只在这里呈现。
    pub fn has_failed_assertions(&self) -> bool {
        self.assertions.iter().any(|a| !a.passed)
    }
}
