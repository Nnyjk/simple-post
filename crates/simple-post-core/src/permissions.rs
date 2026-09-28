//! Permission gating for Simple Post commands and MCP tools.
//!
//! Two parallel layers coexist here:
//!
//! 1. **Per-project [`Permission`]** (Read / ReadCall / Full) — the
//!    historical gate. Stored in the `agent_permissions` table. The
//!    MCP tool handler's [`check_permission`] wrapper consults this
//!    table before running a mutating tool. **`required_permission` is
//!    preserved unchanged for back-compat** — new code should prefer
//!    the global [`CapabilityPolicy`] for "should this tool be exposed /
//!    called by the agent at all" and fall through to per-project
//!    [`Permission`] for "is this specific project writable".
//!
//! 2. **Global [`CapabilityPolicy`]** (ReadOnly / SafeWrite /
//!    FullAccess) — a higher-order gate configured by the user in the
//!    settings panel. Stored in `preferences.mcp.policy`. Drives the
//!    `tools/list` visibility AND a second pre-flight check on every
//!    `tools/call`. A "Read" per-project permission is irrelevant if
//!    the global policy is "SafeWrite" and vice versa — we take the
//!    conservative max (a stricter side wins).
//!
//! The [`Permission`] enum lives in `crate::domain` (mirrors
//! `src/types/domain.ts`); this module re-exports it so callers can do
//! `use crate::permissions::Permission` without depending on `domain`
//! directly. [`CapabilityPolicy`] / [`Capability`] live here because
//! they're MCP-specific (the `permissions` table is shared with the
//! renderer's per-project permission form; the capability layer is not).

use serde::{Deserialize, Serialize};

/// Re-export so `use crate::permissions::Permission` resolves to the
/// single canonical type defined in `crate::domain`. Keeps call sites
/// in commands.rs / mcp/ decoupled from `domain`'s exact location while
/// still sharing one definition across the crate.
pub use crate::domain::Permission;

// ===========================================================================
// Global MCP capability policy — user-facing three-tier switch
// ===========================================================================

/// Coarse-grained, **global** capability tier the user selects in the
/// MCP settings panel. Defaults to [`CapabilityPolicy::ReadOnly`] —
/// safe-by-default; the AI cannot mutate anything until the user
/// explicitly opts in.
///
/// The wire format is snake_case (`"read_only"` / `"safe_write"` /
/// `"full_access"`) — matches the UI copy and the persistence layer
/// in `preferences::mcp_policy`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPolicy {
    /// AI can read state. `tools/list` exposes only Read-class tools;
    /// `tools/call` for anything else returns
    /// `AppError::PermissionDenied` BEFORE invoking the handler.
    #[default]
    ReadOnly,
    /// AI can read + create/update non-deletable resources. `Draft`
    /// suffix omitted on purpose — keeps wire form short.
    SafeWrite,
    /// AI can do anything the `permissions` table allows (including
    /// destructive delete).
    FullAccess,
}

impl CapabilityPolicy {
    /// Wire string. Stable across releases — `preferences` persists
    /// this exact form.
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityPolicy::ReadOnly => "read_only",
            CapabilityPolicy::SafeWrite => "safe_write",
            CapabilityPolicy::FullAccess => "full_access",
        }
    }

    /// Inverse of [`Self::as_str`]. Used by `preferences::mcp_policy`
    /// to map a stored value back to the enum. Unknown / empty values
    /// return `None`; the caller is expected to substitute
    /// [`CapabilityPolicy::default`] in that case.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(CapabilityPolicy::ReadOnly),
            "safe_write" => Some(CapabilityPolicy::SafeWrite),
            "full_access" => Some(CapabilityPolicy::FullAccess),
            _ => None,
        }
    }

    /// True iff this policy tier covers the [`Capability`] class. Total
    /// order Read < Create < Update < Delete < Send (with Delete at the
    /// same level as Send — both are destructive / outbound).
    ///
    /// Note: Send is at the top because `api_run_request` /
    /// `api_call_endpoint` literally make outbound HTTP calls — the
    /// capability tier "let the AI make network calls" is the highest
    /// trust we grant.
    pub fn satisfies(&self, required: Capability) -> bool {
        let rank_required = capability_rank(required);
        let rank_self = match self {
            CapabilityPolicy::ReadOnly => capability_rank(Capability::Read),
            CapabilityPolicy::SafeWrite => capability_rank(Capability::Update),
            CapabilityPolicy::FullAccess => capability_rank(Capability::Delete),
        };
        // Special case: SafeWrite does NOT cover Send (network calls).
        // Only FullAccess covers Send. This is the "safe write ≠
        // free-form outbound HTTP" boundary the spec calls out.
        if required == Capability::Send {
            return matches!(self, CapabilityPolicy::FullAccess);
        }
        rank_self >= rank_required
    }
}

/// Capability class a single tool requires. Used by the global
/// capability gate (the per-project `permissions` table uses a
/// different [`Permission`] enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// Pure observation — no network, no state change.
    Read,
    /// New resource created.
    Create,
    /// Existing resource mutated in place.
    Update,
    /// Resource removed (destructive).
    Delete,
    /// Outbound HTTP call (`api_run_request`, `api_call_endpoint`).
    /// Highest trust required because it touches third-party systems.
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

/// Map a tool name to the minimum permission it demands.
///
/// Anything **not** listed defaults to [`Permission::Full`]. This is the
/// safe-by-default direction — a new write tool added without an entry
/// here will require Full, never Read.
pub fn required_permission(tool_name: &str) -> Permission {
    match tool_name {
        // Read — pure inspection, no network.
        "env_list"
        | "env_get_active"
        | "project_list"
        | "project_get"
        | "module_list"
        | "collection_list"
        | "endpoint_list"
        | "endpoint_get"
        | "endpoint_search"
        | "base_url_def_list"
        | "api_resolve_template"
        | "api_resolve_endpoint"
        | "api_validate_endpoint"
        | "api_parse_response"
        | "api_diff_responses"
        | "api_extract_jsonpath" => Permission::Read,

        // ReadCall — Read + actually calling an external service.
        "api_run_request" | "api_call_endpoint" => Permission::ReadCall,

        // Full — default for any write / mutating operation. Keep new
        // write tools OUT of the match above; let them fall here.
        _ => Permission::Full,
    }
}

/// Look up the project's stored permission level and compare it
/// against the level a tool requires.
///
/// Callers must be inside a `with_conn`/`with_tx` closure (or otherwise
/// hold a `&mut Connection`). On deny, returns
/// [`crate::error::AppError::PermissionDenied`].
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

/// True iff `actual` is at least as capable as `required`. Total order
/// `Read < ReadCall < Full`.
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

// ===========================================================================
// resolve_capability — merge global policy with per-project permission
// ===========================================================================

/// Resolve the **effective** capability policy that should gate the
/// next MCP tool call. Two inputs:
///
/// 1. The global `mcp.policy` preference (the three-tier switch).
/// 2. The per-project `permissions` row (back-compat layer that has
///    lived in the schema since v0.1).
///
/// `project_id == None` ⇒ only the global preference matters
/// (e.g. `tools/list` builds the user-visible tool inventory without
/// touching any project).
///
/// `project_id == Some(_)` ⇒ both sides are consulted and we take the
/// **stricter** of the two. Rationale: if the global tier is
/// "ReadOnly" but the project grants `Full`, the project grant does
/// not overrule the user's choice to keep AI write actions globally
/// off. Symmetrically, if the global tier is "FullAccess" but the
/// project is only `ReadCall`, the project's restrictive grant wins.
/// Conservative direction is the only safe direction.
///
/// Returns an `AppError::Db` on storage failure (preferences or
/// permissions lookup). Read-only ops use this; callers should treat
/// the error as a deny (don't silently fall back to a permissive
/// default).
pub async fn resolve_capability(
    pool: &crate::db::DbPool,
    project_id: Option<&str>,
) -> Result<CapabilityPolicy, crate::error::AppError> {
    // 1. global tier — pull from preferences (may be missing on first
    //    launch, in which case default = ReadOnly).
    let pool_clone = pool.clone();
    let global = tokio::task::spawn_blocking(move || {
        crate::db::with_conn(&pool_clone, crate::db::preferences::mcp_policy)
    })
    .await
    .map_err(|e| crate::error::AppError::Db(format!("preferences task: {e}")))??;

    // 2. per-project permission — only consulted when a project is in
    //    scope. Map the per-project Permission onto a capability tier
    //    so the two layers compose:
    //      Permission::Read       → ReadOnly
    //      Permission::ReadCall   → SafeWrite (ReadCall can make outbound
    //                               HTTP via the api_* tools)
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

    // 3. Conservative max — whichever side is stricter wins.
    Ok(match project_policy {
        None => global,
        Some(pp) => stricter(global, pp),
    })
}

/// Project `Permission` → the closest matching `CapabilityPolicy`.
fn permission_to_capability(p: Permission) -> CapabilityPolicy {
    match p {
        Permission::Read => CapabilityPolicy::ReadOnly,
        Permission::ReadCall => CapabilityPolicy::SafeWrite,
        Permission::Full => CapabilityPolicy::FullAccess,
    }
}

/// Pick the stricter of two policies. Same variant ⇒ that variant;
/// otherwise rank by capability ceiling and pick the **lower** one —
/// conservative-wins semantics. A user who set the global policy to
/// `ReadOnly` doesn't want a per-project `Full` to silently re-enable
/// writes, and vice versa.
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
