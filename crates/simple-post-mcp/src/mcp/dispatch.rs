//! `tools/call` 的分派 —— 按名字找 handler，再过一次全局能力门。

use serde_json::Value;
use simple_post_core::permissions::resolve_capability;

use super::protocol::CallToolResult;
use super::registry::required_capability;
use super::state::AppState;
use super::tool_table;

/// 按名字 dispatch 一次 `tools/call`。未知名字 → `AppError::NotFound`
/// （JSON-RPC 层折成 `is_error: true`，LLM 能看到缺的是哪个工具）。
///
/// 进 handler 前再过一次全局能力门（`tools/list` 那道在 `tool_descriptors`）。
/// 策略为 `ReadOnly` 时直接 `tools/call` 一个写工具，拿到的是
/// `AppError::PermissionDenied`，handler 根本不会执行。
pub async fn dispatch(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<CallToolResult, simple_post_core::error::AppError> {
    // 先解析 handler：未注册的名字必须回 `NotFound`，而不是被能力门掩盖成
    // `PermissionDenied`。
    let entry = tool_table()
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| {
            simple_post_core::error::AppError::NotFound(format!("tool '{name}' not registered"))
        })?;

    // 能力预检：策略取法与 `tool_descriptors` 完全一致（此处无项目维度，只有
    // 全局偏好），这样不复查 `tools/list` 就直接 `tools/call` 的客户端同样
    // 无法改动任何东西。缺行 = 拒绝：缺行永远是注册表 bug，不是隐式放行。
    match required_capability(name) {
        Some(req) => {
            let policy = resolve_capability(&state.pool, None).await?;
            if !policy.satisfies(req) {
                return Err(simple_post_core::error::AppError::PermissionDenied(
                    format!(
                        "tool '{name}' requires {:?}, current policy is {:?}",
                        req, policy
                    ),
                ));
            }
        }
        None => {
            return Err(simple_post_core::error::AppError::PermissionDenied(
                format!(
                    "tool '{name}' has no capability classification; \
                     refusing to execute (registry bug — add it to REQUIRED_CAPABILITY)"
                ),
            ));
        }
    }
    (entry.func)(state, args).await
}
