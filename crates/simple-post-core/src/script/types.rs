//! 沙箱内部的数据形状：进去的（请求 / 响应 / 环境）与出来的（改过的请求）。
//!
//! 线上类型（脚本结果、断言、日志、错误、环境变量写入）在 [`crate::domain::script`] ——
//! 它们要跨 IPC 与 MCP 边界，故与渲染层的 TS 镜像放在一起。

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
