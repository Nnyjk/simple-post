//! 请求脚本的沙箱：用户写的 JavaScript 在这里跑。
//!
//! 形状是「一段固定 JS 前导 + 一次 JSON 数据进出」：宿主把请求 / 响应 / 环境序列化进去，
//! 脚本跑完，宿主把改动（改过的请求、变量写入、日志、断言）读回来。**Rust 侧不向引擎
//! 注册任何回调函数**，故宿主能力就等于前导里显式给出的那几样，加上没有任何 I/O 的
//! QuickJS 标准库——「脚本够不到文件系统 / 网络 / 任意进程」不靠逐个禁用，而靠根本没有
//! 那条路。
//!
//! 三条硬约束，改动本模块时必须守住：
//!
//! * **不得 panic。** release profile 是 `panic = "abort"`，脚本路径上任何 panic 都会直接
//!   终止进程，而宿主（MCP 守护进程）正替别人握着连接。
//! * **不得写 stdout。** `simple-post-mcp` 的 stdout 只允许 JSON-RPC 帧，而本模块被它
//!   链接。`console.log` 走内存里的日志数组。
//! * **不得在 tokio worker 上直接 eval。** 引擎是同步 CPU 密集的，整条链要在一个
//!   `spawn_blocking` 闭包里跑完（见 [`send::send_with_scripts`]）。
//!
//! 限额在 [`DEFAULT_SCRIPT_LIMITS`]：墙钟截止、内存、栈、日志与断言条数。墙钟是**真
//! 打断**——引擎的中断处理被触发时抛不可捕获的异常，死循环不会把宿主拖住。

mod chain;
mod engine;
mod limits;
mod prelude;
mod run;
mod send;
mod transform;
mod types;

// 线上类型住在 `domain`（要与渲染层的 TS 镜像放在一起），这里转出一次，
// 让 `simple_post_core::script::` 一个入口拿全。
pub use crate::domain::{
    AssertionResult, EnvSnapshot, EnvWrite, ScriptError, ScriptErrorKind, ScriptLog, ScriptOutcome,
    ScriptPhase,
};
pub use chain::{collect_chain, merge_env};
pub use engine::{Sandbox, ScriptEffects, ScriptFailure, ScriptRun, TransformRun};
pub use limits::{ScriptLimits, DEFAULT_SCRIPT_LIMITS};
pub use run::run_phase;
pub use send::{send_with_scripts, ScriptOverride, ScriptedSendInput, ScriptedSendOutcome};
pub use transform::{run_transform, TransformOutcome};
pub use types::{ScriptRequest, ScriptResponse, ScriptSource, ScriptUnit};

// 测试。兄弟 `#[path]` 模块，遵循本 crate 的惯例：Rust 按模块路径找文件，测试专用
// *目录* 是找不到的。

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_tests;

#[cfg(test)]
#[path = "run_test.rs"]
mod run_tests;

#[cfg(test)]
#[path = "stdout_test.rs"]
mod stdout_tests;

#[cfg(test)]
#[path = "chain_test.rs"]
mod chain_tests;

#[cfg(test)]
#[path = "send_test.rs"]
mod send_tests;
