//! 处理脚本：把抽取出来的值再加工一遍。
//!
//! 这一步在**抽取之后**：JSONPath 负责「从响应里取出哪一段」，脚本负责「这一段怎么
//! 变成变量值」。JSONPath 表达不了的加工（取数组第一项、拼接、算账、换字段名）交给
//! 脚本，故 [`crate::script`] 那套沙箱被复用到这里。
//!
//! 脚本看到的输入：
//!
//! - `pm.data` —— 抽取出来的那个值；
//! - `pm.response` —— 取回来的原始响应（`text()` / `json()`）；
//! - `pm.environment` —— 取数用的那个环境。
//!
//! 脚本 `return` 的值就是最终值。没有脚本时，抽取结果原样通过。

use serde_json::Value;

use crate::domain::{DynamicError, DynamicErrorKind, DynamicVariable};
use crate::script::{self, EnvSnapshot, ScriptResponse, DEFAULT_SCRIPT_LIMITS};

use super::fetcher::RawSource;
use super::source::SourceContext;

/// 跑一段处理脚本。
///
/// 没有脚本（空串或空白）→ 原样返回抽取结果。
///
/// 脚本失败 → 返回一条错误：调用方按「取值失败」处理（缺失，或退回过期缓存值）。
pub async fn apply(
    def: &DynamicVariable,
    extracted: Value,
    raw: &RawSource,
    source: &SourceContext,
) -> Result<Value, DynamicError> {
    let Some(script) = def.transform_script.as_deref() else {
        return Ok(extracted);
    };
    if script.trim().is_empty() {
        return Ok(extracted);
    }

    let response = ScriptResponse {
        code: raw.status,
        status: raw.status.to_string(),
        headers: raw.headers.clone(),
        body: raw.body.clone(),
        // 动态变量的取数响应只有文本形态：抽取规则是 JSONPath，它读的是解析后的 JSON。
        body_base64: None,
        response_time_ms: raw.duration_ms,
    };
    let env = EnvSnapshot {
        id: source.env.as_ref().map(|e| e.id.clone()),
        variables: source
            .env
            .as_ref()
            .map(|e| {
                e.variables
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    };

    let outcome = script::run_transform(
        script,
        extracted,
        Some(response),
        env,
        DEFAULT_SCRIPT_LIMITS,
    )
    .await;
    for log in &outcome.logs {
        tracing::debug!("dynamic: 变量 '{}' 的处理脚本：{}", def.name, log.message);
    }
    match outcome.failure {
        Some(failure) => Err(DynamicError {
            name: def.name.clone(),
            kind: DynamicErrorKind::ScriptError,
            message: failure.message,
        }),
        None => Ok(outcome.value.unwrap_or(Value::Null)),
    }
}
