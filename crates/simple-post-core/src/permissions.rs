//! Simple Post 命令与 MCP 工具的权限门。
//!
//! 此处并存两层：
//!
//! 1. **逐项目 [`Permission`]**（Read / ReadCall / Full）——历史那一道门，存在
//!    `agent_permissions` 表里。MCP 工具 handler 的 [`require`] 包装器在跑一个会改动的工具
//!    前查这张表。新的代码应优先用全局 [`CapabilityPolicy`] 回答「这个工具该不该被 agent
//!    看到/调用」，再落到逐项目 [`Permission`] 回答「这个具体项目可不可写」。
//! 2. **全局 [`CapabilityPolicy`]**（ReadOnly / SafeWrite / FullAccess）——用户在设置面板里
//!    配的更高阶一道门，存在 `preferences.mcp.policy`。它同时驱动 `tools/list` 的可见性与
//!    每次 `tools/call` 上的第二道预检。逐项目是 "Read" 而全局是 "SafeWrite" 无关紧要，反之
//!    亦然——我们取保守的较大者（更严的一侧胜出）。
//!
//! [`Permission`] 枚举住在 `crate::domain`（镜像 `src/types/domain.ts`），本模块重导出它，
//! 好让调用方写 `use crate::permissions::Permission` 而不必直接依赖 `domain`。
//! [`CapabilityPolicy`] / [`Capability`] 住在这里，因为它们是 MCP 专属的（`permissions` 表与
//! 渲染层的逐项目权限表单共享，而能力层不共享）。

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
    /// （Delete 与 Send 同级——两者都是破坏性/外发的）。
    /// 注意 Send 在最顶，因为 `api_run_request` / `api_call_endpoint` 真的会发出站 HTTP
    /// 请求——「让 AI 发网络请求」是我们授予的最高信任级别。
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

/// 把工具名映射到它所需的最低权限。
/// **未列出**的一切都默认为 [`Permission::Full`]。这是安全的方向——一个没有在此登记就
/// 加进来的新写入工具会要求 Full，绝不会是 Read。
pub fn required_permission(tool_name: &str) -> Permission {
    match tool_name {
        // Read——纯检查，无网络。
        "env_list"
        | "env_get_active"
        | "project_list"
        | "project_get"
        | "module_list"
        | "collection_list"
        | "endpoint_list"
        | "endpoint_get"
        | "endpoint_search"
        | "api_resolve_template"
        | "api_resolve_endpoint"
        | "api_validate_endpoint"
        | "api_parse_response"
        | "api_diff_responses"
        | "api_extract_jsonpath"
        | "dynamic_var_list"
        | "dynamic_var_get" => Permission::Read,

        // ReadCall——Read 加上真的去调一个外部服务。
        "api_run_request" | "api_call_endpoint" | "dynamic_var_resolve" => Permission::ReadCall,

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

// resolve_capability——把全局策略与逐项目权限合并

/// 解析应当给下一次 MCP 工具调用把关的**有效**能力策略。两个输入：
/// 1. 全局的 `mcp.policy` 偏好（三档开关）。
/// 2. 逐项目的 `permissions` 行（自 v0.1 起就在 schema 里的向后兼容层）。
///
/// `project_id == None` ⇒ 只有全局偏好要紧（例如 `tools/list` 在不碰任何项目的前提下
/// 构建用户可见的工具清单）。
/// `project_id == Some(_)` ⇒ 两侧都问，取二者中**更严**的。理由：全局档位是 "ReadOnly"
/// 而项目授予 `Full` 时，项目那份授予不能推翻用户「把 AI 写操作全局关掉」的选择；反过来，
/// 全局是 "FullAccess" 而项目只有 `ReadCall` 时，项目那份限制性授予胜出。保守的方向是唯一
/// 安全的方向。
/// 存储失败（preferences 或 permissions 查询）时返回 `AppError::Db`。只读操作用它；
/// 调用方应把该错误当作拒绝（不要静默退回一个从宽的默认值）。
pub async fn resolve_capability(
    pool: &crate::db::DbPool,
    project_id: Option<&str>,
) -> Result<CapabilityPolicy, crate::error::AppError> {
    // 1. 全局档位——取自 preferences（首次启动可能缺失，此时默认 = ReadOnly）。
    let pool_clone = pool.clone();
    let global = tokio::task::spawn_blocking(move || {
        crate::db::with_conn(&pool_clone, crate::db::preferences::mcp_policy)
    })
    .await
    .map_err(|e| crate::error::AppError::Db(format!("preferences task: {e}")))??;

    // 2. 逐项目权限——只在有项目在范围内时才问。把逐项目 `Permission` 映射到能力档位，
    //    使两层可组合：
    //      Permission::Read       → ReadOnly
    //      Permission::ReadCall   → SafeWrite（ReadCall 可以经 api_* 工具发外站 HTTP）
    //      Permission::Full       → FullAccess
    let project_policy = match project_id {
        Some(pid) => {
            let pid = pid.to_string();
            let pool_clone = pool.clone();
            let perm = tokio::task::spawn_blocking(move || {
                crate::db::with_conn(&pool_clone, |c| crate::db::permissions::get(c, &pid))
            })
            .await
            .map_err(|e| crate::error::AppError::Db(format!("permissions task: {e}")))??;
            Some(permission_to_capability(perm))
        }
        None => None,
    };

    // 3. 保守取严——更严的一侧胜出。
    Ok(match project_policy {
        None => global,
        Some(pp) => stricter(global, pp),
    })
}

/// 逐项目 `Permission` → 最接近的 `CapabilityPolicy`。
fn permission_to_capability(p: Permission) -> CapabilityPolicy {
    match p {
        Permission::Read => CapabilityPolicy::ReadOnly,
        Permission::ReadCall => CapabilityPolicy::SafeWrite,
        Permission::Full => CapabilityPolicy::FullAccess,
    }
}

/// 取两个策略中更严的一个。同一变体即该变体；否则按能力上限排名取**低**的那个——
/// 保守胜出。把全局策略设成 `ReadOnly` 的用户不希望某个逐项目的 `Full` 静默重新打开写入，
/// 反之亦然。
fn stricter(a: CapabilityPolicy, b: CapabilityPolicy) -> CapabilityPolicy {
    let rank = |p: CapabilityPolicy| match p {
        CapabilityPolicy::ReadOnly => 0,
        CapabilityPolicy::SafeWrite => 1,
        CapabilityPolicy::FullAccess => 2,
    };
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

#[cfg(test)]
#[path = "permissions_test.rs"]
mod tests;
