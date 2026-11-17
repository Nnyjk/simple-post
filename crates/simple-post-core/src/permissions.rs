//! Simple Post 命令与 MCP 工具的权限门。
//!
//! 此处并存两层：
//!
//! 1. **全局 [`CapabilityPolicy`]**（ReadOnly / SafeWrite / FullAccess）——用户在设置面板里配的
//!    高阶一道门，存在 `preferences.mcp.policy`。它同时驱动 `tools/list` 的可见性与每次
//!    `tools/call` 上的第二道预检。
//! 2. **逐项目 [`Permission`]**（Read / ReadCall / Full）——存在 `agent_permissions` 表里。
//!    MCP 工具 handler 的 [`require`] 包装器在跑一个会改动的工具前查这张表。
//!
//! 两层各答各的问题，互不推演：全局 [`CapabilityPolicy`] 回答「这个工具该不该被 agent 看到 /
//! 调用」（`tools/list` 的可见性与每次 `tools/call` 的预检，见 [`resolve_capability`]）；
//! 逐项目 [`Permission`] 回答「这个具体项目可不可写」，由已经知道项目 id 的 handler 调
//! [`require`]（MCP 侧包装成 `check_permission`）查 `agent_permissions` 表——项目级裁决
//! 只发生在那里。
//!
//! [`Permission`] 枚举住在 `crate::domain`（镜像 `src/types/domain.ts`），本模块重导出它。
//! [`CapabilityPolicy`] / [`Capability`] 住在这里，因为它们是 MCP 专属的。

use serde::{Deserialize, Serialize};

/// 重导出，使 `use crate::permissions::Permission` 解析到 `crate::domain` 里那唯一权威的
/// 类型定义。它让 `commands/` / mcp/ 里的调用点不必知道 `domain` 的确切位置，
/// 同时全 crate 仍共享一份定义。
pub use crate::domain::Permission;

// 全局 MCP 能力策略——面向用户的三档开关

/// 用户在 MCP 设置面板里选的粗粒度**全局**能力档位。默认 [`CapabilityPolicy::ReadOnly`]——
/// 默认安全；在用户显式 opt in 之前，AI 不能改动任何东西。
/// 线格式是 snake_case（`"read_only"` / `"safe_write"` / `"full_access"`）——与 UI 文案
/// 以及 `preferences::mcp_policy` 里的持久化一致。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPolicy {
    /// AI 可读状态。`tools/list` 只暴露 Read 类工具；其余的 `tools/call` 在调用 handler
    /// **之前**就返回 `AppError::PermissionDenied`。
    #[default]
    ReadOnly,
    /// AI 可读 + 创建/更新不可删除的资源。刻意不加 `Draft` 后缀——保持线格式短。
    SafeWrite,
    /// AI 可做 `permissions` 表允许的任何事（含破坏性的删除）。
    FullAccess,
}

impl CapabilityPolicy {
    /// 线字符串。跨版本稳定——`preferences` 持久化的就是这个形态。
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityPolicy::ReadOnly => "read_only",
            CapabilityPolicy::SafeWrite => "safe_write",
            CapabilityPolicy::FullAccess => "full_access",
        }
    }

    /// [`Self::as_str`] 的逆。由 `preferences::mcp_policy` 把存下的值映射回枚举。
    /// 未知/空值返回 `None`；调用方应在该情况下改用 [`CapabilityPolicy::default`]。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(CapabilityPolicy::ReadOnly),
            "safe_write" => Some(CapabilityPolicy::SafeWrite),
            "full_access" => Some(CapabilityPolicy::FullAccess),
            _ => None,
        }
    }

    /// 本档位是否覆盖某个 [`Capability`] 类。全序 Read < Create < Update < Delete < Send
    /// （Delete 与 Send 同级）。Send 要求最高档位，因为它真的会发出站 HTTP 请求。
    pub fn satisfies(&self, required: Capability) -> bool {
        let rank_required = capability_rank(required);
        let rank_self = match self {
            CapabilityPolicy::ReadOnly => capability_rank(Capability::Read),
            CapabilityPolicy::SafeWrite => capability_rank(Capability::Update),
            CapabilityPolicy::FullAccess => capability_rank(Capability::Delete),
        };
        // 特例：SafeWrite **不**覆盖 Send（网络调用）。只有 FullAccess 覆盖 Send。
        // 这就是规格点明的「安全写 ≠ 自由外发 HTTP」边界。
        if required == Capability::Send {
            return matches!(self, CapabilityPolicy::FullAccess);
        }
        rank_self >= rank_required
    }
}

/// 单个工具所需的 capability 类。供全局能力门使用（逐项目的 `permissions` 表用的是另一个
/// [`Permission`] 枚举）。
// 不派生 `PartialOrd`/`Ord`：那会把 `Send` 排在 `Delete` 之上，而实际用的
// `capability_rank` 让两者同级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// 纯观察——无网络，无状态变化。
    Read,
    /// 新建资源。
    Create,
    /// 原地改动已有资源。
    Update,
    /// 移除资源（破坏性）。
    Delete,
    /// 外发 HTTP 调用（`api_run_request`、`api_call_endpoint`）。所需信任最高，
    /// 因为它触及第三方系统。
    Send,
}

fn capability_rank(c: Capability) -> u8 {
    match c {
        Capability::Read => 0,
        Capability::Create => 1,
        Capability::Update => 2,
        Capability::Delete => 3,
        Capability::Send => 3,
    }
}

/// 把工具名映射到它所需的最低权限。**未列出**的一切都默认为 [`Permission::Full`]：一个没有
/// 在此登记就加进来的新写入工具会要求 Full，绝不会是 Read。
pub fn required_permission(tool_name: &str) -> Permission {
    match tool_name {
        // Read——纯检查，无网络。
        "env_list"
        | "env_get_default"
        | "project_list"
        | "project_get"
        | "module_list"
        | "collection_list"
        | "endpoint_list"
        | "endpoint_get"
        | "endpoint_search"
        | "endpoint_history"
        | "api_resolve_template"
        | "api_resolve_endpoint"
        | "api_validate_endpoint"
        | "api_parse_response"
        | "api_diff_responses"
        | "api_extract_jsonpath"
        | "dynamic_var_list"
        | "dynamic_var_get" => Permission::Read,

        // ReadCall——Read 加上真的去调一个外部服务。
        // `api_run_collection` 在这一档：它逐个调用端点，每个端点都是一次出站请求。
        "api_run_request" | "api_call_endpoint" | "api_run_collection" | "dynamic_var_resolve" => {
            Permission::ReadCall
        }

        // Full——任何写入/改动操作的默认值。新的写入工具**不要**加到上面的匹配里，
        // 让它们落到这里。
        _ => Permission::Full,
    }
}

/// 查项目存下的权限级别，并与之工具所需的级别比较。
/// 调用方必须在一个 `with_conn`/`with_tx` 闭包内（或以别的方式持有 `&mut Connection`）。
/// 拒绝时返回 [`crate::error::AppError::PermissionDenied`]。
pub fn require(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    required: Permission,
) -> Result<(), crate::error::AppError> {
    let actual = crate::db::permissions::get(conn, project_id)?;
    if permission_at_least(actual, required) {
        Ok(())
    } else {
        Err(crate::error::AppError::PermissionDenied(format!(
            "project '{}' permission is '{}', required '{}'",
            project_id,
            actual.as_str(),
            required.as_str()
        )))
    }
}

/// `actual` 的能力是否至少与 `required` 相当。全序 `Read < ReadCall < Full`。
pub fn permission_at_least(actual: Permission, required: Permission) -> bool {
    rank(actual) >= rank(required)
}

fn rank(p: Permission) -> u8 {
    match p {
        Permission::Read => 0,
        Permission::ReadCall => 1,
        Permission::Full => 2,
    }
}

// 全局能力档——preferences 里存的那一档

/// 读出下一次 MCP 工具调用要用的**全局**能力档（`preferences.mcp.policy`）。
///
/// 这里只回答「用户的全局开关允许多大权限」，`tools/list` 的可见性与 `tools/call` 的预检
/// 都用这同一个答案。项目级裁决不在这里：它由知道项目 id 的 handler 用 [`require`] 查
/// `agent_permissions` 表完成。
///
/// 读库放进 `spawn_blocking`：`db::with_conn` 在库忙时会重试并等待，那段等待不该占住一个
/// Tokio 工作线程。缺行或存了不认识的值 ⇒ [`CapabilityPolicy::default`]（ReadOnly）。
/// 读取失败返回 `AppError::Db`，调用方应把该错误当作拒绝。
pub async fn resolve_capability(
    pool: &crate::db::DbPool,
) -> Result<CapabilityPolicy, crate::error::AppError> {
    let pool_clone = pool.clone();
    tokio::task::spawn_blocking(move || {
        crate::db::with_conn(&pool_clone, crate::db::preferences::mcp_policy)
    })
    .await
    .map_err(|e| crate::error::AppError::Db(format!("preferences task: {e}")))?
}

#[cfg(test)]
#[path = "permissions_test.rs"]
mod tests;
