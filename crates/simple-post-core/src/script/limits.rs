//! 沙箱的资源上限。
//!
//! 上限是**每阶段**一份预算（整条脚本链共享），不是每脚本一份：interrupt handler 的
//! 截止时刻在 Runtime 建立时固定，一个阶段一个 Runtime，语义清楚且实现简单。

use serde::{Deserialize, Serialize};

/// 一份限额。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLimits {
    /// 墙钟预算（毫秒）。超时的脚本被引擎中断，不是等它自己跑完。
    pub timeout_ms: u64,
    /// 内存上限（字节）。
    pub memory_bytes: usize,
    /// 栈上限（字节）。
    pub stack_bytes: usize,
    /// 收集的日志条数上限。超出的丢弃，防止脚本狂打日志撑爆响应体。
    pub max_logs: usize,
    /// 收集的断言条数上限。
    pub max_assertions: usize,
}

/// 默认限额。
///
/// 1000 ms 对「读一个变量、塞一个 token」这类脚本绰绰有余，而死循环脚本不会把宿主
/// 拖住一秒以上。内存 32 MB 足以跑常规脚本，又不至于让一个失控脚本吃掉整台机器。
pub const DEFAULT_SCRIPT_LIMITS: ScriptLimits = ScriptLimits {
    timeout_ms: 1_000,
    memory_bytes: 32 * 1024 * 1024,
    stack_bytes: 256 * 1024,
    max_logs: 500,
    max_assertions: 200,
};
