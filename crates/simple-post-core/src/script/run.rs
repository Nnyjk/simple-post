//! 跑一整条脚本链的一个阶段。
//!
//! 链上的多段脚本在**同一个沙箱**里顺序执行，故它们共享全局——collection 脚本定义的
//! 辅助函数，端点脚本能直接用。
//!
//! 失败语义：某段脚本失败 → **它对请求的改动被丢弃**（请求回到那段脚本执行前的样子），
//! 它写下的环境变量也不落库；日志与断言作为诊断保留；链条继续跑下一段。

use super::engine::Sandbox;
use super::limits::ScriptLimits;
use super::types::{EnvSnapshot, ScriptRequest, ScriptResponse, ScriptUnit};
use crate::domain::{ScriptError, ScriptOutcome, ScriptPhase};

/// 跑一个阶段。
///
/// `request` 就地更新为脚本改写后的形状；`response` 只在 test 阶段给脚本看。
pub fn run_phase(
    phase: ScriptPhase,
    units: &[ScriptUnit],
    request: &mut ScriptRequest,
    response: Option<&ScriptResponse>,
    env: &EnvSnapshot,
    limits: &ScriptLimits,
) -> ScriptOutcome {
    let mut outcome = ScriptOutcome::empty(phase);

    let scripts: Vec<(&str, &str)> = units
        .iter()
        .filter_map(|unit| {
            let source = match phase {
                ScriptPhase::Pre => unit.pre_request.as_deref(),
                ScriptPhase::Test => unit.test.as_deref(),
            };
            // 空白脚本视作「没有脚本」，不进链。
            source
                .filter(|text| !text.trim().is_empty())
                .map(|text| (unit.source.label(), text))
        })
        .collect();
    if scripts.is_empty() {
        return outcome;
    }

    let sandbox = match Sandbox::new(limits) {
        Ok(sandbox) => sandbox,
        Err(failure) => {
            outcome.errors.push(ScriptError {
                kind: failure.kind,
                message: failure.message,
                source: String::new(),
            });
            return outcome;
        }
    };
    if let Err(failure) = sandbox.init(request, response, env, limits) {
        outcome.errors.push(ScriptError {
            kind: failure.kind,
            message: failure.message,
            source: String::new(),
        });
        return outcome;
    }

    for (label, script) in scripts {
        let run = sandbox.run(script, request);
        outcome.logs.extend(run.effects.logs);
        outcome.assertions.extend(run.effects.assertions);
        outcome.env_writes.extend(run.effects.env_writes);
        match run.failure {
            Some(failure) => outcome.errors.push(ScriptError {
                kind: failure.kind,
                message: failure.message,
                source: label.to_string(),
            }),
            // 成功才应用改动。失败时 `effects.request` 已是回滚后的形状，不动它。
            None => *request = run.effects.request,
        }
    }

    outcome
}
