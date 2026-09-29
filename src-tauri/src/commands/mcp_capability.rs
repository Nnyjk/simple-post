// =========================================================================
// MCP capability policy (commit 1: feat(mcp): three-tier capability policy)
// =========================================================================
//
// Three Tauri commands + matching REST endpoints (see `web_api.rs`).
// The capability policy is the **global** lever that filters `tools/list`
// for the AI; per-project permissions in the `permissions` table still
// gate individual mutations. Both layers fail closed, and an unregistered
// tool is treated as unclassified rather than permitted.

use crate::db::DbPool;
use crate::error::AppError;
use crate::permissions::CapabilityPolicy;

use super::dto::McpCapabilityDto;

/// 返回当前全局能力策略，形状与 Web REST 完全一致。
#[tauri::command]
pub async fn mcp_capability_get(
    pool: tauri::State<'_, DbPool>,
) -> Result<McpCapabilityDto, AppError> {
    let policy = crate::db::with_conn(pool.inner(), crate::db::preferences::mcp_policy)?;
    Ok(McpCapabilityDto { policy })
}

/// 更新全局能力策略并返回与 Web REST 相同的对象。
#[tauri::command]
pub async fn mcp_capability_set(
    policy: String,
    pool: tauri::State<'_, DbPool>,
) -> Result<McpCapabilityDto, AppError> {
    let parsed = CapabilityPolicy::parse(&policy).ok_or_else(|| {
        AppError::Validation(format!(
            "unknown mcp policy '{policy}' (expected read_only | safe_write | full_access)"
        ))
    })?;
    crate::db::with_conn(pool.inner(), |c| {
        crate::db::preferences::set_mcp_policy(c, parsed)
    })?;
    Ok(McpCapabilityDto { policy: parsed })
}
/// 返回当前能力策略下可见的工具名称。
#[tauri::command]
pub async fn mcp_capability_list_tools(
    pool: tauri::State<'_, DbPool>,
) -> Result<Vec<String>, AppError> {
    use std::sync::Arc;
    use std::time::Instant;
    let state = crate::mcp::AppState {
        pool: pool.inner().clone(),
        started_at: Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    };
    Ok(crate::mcp::tool_descriptors(&state)
        .await
        .into_iter()
        .map(|t| t.name)
        .collect())
}
