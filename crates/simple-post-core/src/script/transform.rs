//! 处理脚本：动态变量取回数据之后，用一段脚本加工它。
//!
//! 与请求脚本的差别只有一处：**它给出一个值，而不是改一个请求**。脚本包在函数体里跑，
//! 故 `return` 就是「最终值」——用户写 `return pm.data.items[0].id;` 即可。
//!
//! 脚本看到的东西：
//!
//! - `pm.data` —— 抽取出来的那个值（JSONPath 抽的那一份）；
//! - `pm.response` —— 取回来的原始响应（`text()` / `json()`）；
//! - `pm.environment` —— 取数用的那个环境。
//!
//! 它写下的环境变量**不落库**：这一步是一次纯计算，产出只有一个值。

use serde_json::Value as Json;

use super::engine::{Sandbox, ScriptFailure};
use super::limits::ScriptLimits;
use super::types::{EnvSnapshot, ScriptRequest, ScriptResponse};
use crate::domain::{ScriptErrorKind, ScriptLog};

/// 一段处理脚本跑完的产物。
pub struct TransformOutcome {
    /// 脚本 `return` 的值。失败时为 `None`。
    pub value: Option<Json>,
    /// 脚本写的日志。失败时也保留——诊断比值更要紧。
    pub logs: Vec<ScriptLog>,
    /// 失败原因。失败时 `value` 为 `None`。
    pub failure: Option<ScriptFailure>,
}

/// 跑一段处理脚本，取回它 `return` 的值。
///
/// **放到阻塞线程上**：引擎是同步且 CPU 密集的，在异步 worker 上跑会卡住整个运行时
/// （与 `send_with_scripts` 里的脚本链同一条纪律）。
pub async fn run_transform(
    script: &str,
    data: Json,
    response: Option<ScriptResponse>,
    env: EnvSnapshot,
    limits: ScriptLimits,
) -> TransformOutcome {
    let script = script.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let sandbox = match Sandbox::new(&limits) {
            Ok(sandbox) => sandbox,
            Err(failure) => {
                return TransformOutcome {
                    value: None,
                    logs: Vec::new(),
                    failure: Some(failure),
                };
            }
        };
        // 处理脚本不改请求，故它看到的是一个空请求。
        let request = ScriptRequest::default();
        if let Err(failure) = sandbox.init(&request, response.as_ref(), &env, Some(&data), &limits)
        {
            return TransformOutcome {
                value: None,
                logs: Vec::new(),
                failure: Some(failure),
            };
        }
        let run = sandbox.run_transform(&script);
        TransformOutcome {
            value: run.value,
            logs: run.logs,
            failure: run.failure,
        }
    });
    match handle.await {
        Ok(outcome) => outcome,
        // 阻塞线程里的 panic。release profile 是 `panic = "abort"`，那里到不了这里；
        // 测试构建下才可能，故仍要给调用方一个能看懂的结果。
        Err(join_error) => TransformOutcome {
            value: None,
            logs: Vec::new(),
            failure: Some(ScriptFailure {
                kind: ScriptErrorKind::Runtime,
                message: format!("the script engine panicked: {join_error}"),
            }),
        },
    }
}
