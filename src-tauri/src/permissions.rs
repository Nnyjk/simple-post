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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPolicy {
    /// AI can read state. `tools/list` exposes only Read-class tools;
    /// `tools/call` for anything else returns
    /// `AppError::PermissionDenied` BEFORE invoking the handler.
    ReadOnly,
    /// AI can read + create/update non-deletable resources. `Draft`
    /// suffix omitted on purpose — keeps wire form short.
    SafeWrite,
    /// AI can do anything the `permissions` table allows (including
    /// destructive delete).
    FullAccess,
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        // Safe-by-default. Until the user explicitly opts in, the AI
        // can only observe. See `docs/mcp-permission-policy.md`.
        Self::ReadOnly
    }
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
        crate::db::with_conn(&pool_clone, |c| crate::db::preferences::mcp_policy(c))
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
                crate::db::with_conn(&pool_clone, |c| {
                    crate::db::permissions::get(c, &pid)
                })
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
mod tests {
    use super::*;

    #[test]
    fn at_least_self_always_holds() {
        assert!(permission_at_least(Permission::Read, Permission::Read));
        assert!(permission_at_least(Permission::ReadCall, Permission::ReadCall));
        assert!(permission_at_least(Permission::Full, Permission::Full));
    }

    #[test]
    fn at_least_higher_covers_lower() {
        assert!(permission_at_least(Permission::ReadCall, Permission::Read));
        assert!(permission_at_least(Permission::Full, Permission::Read));
        assert!(permission_at_least(Permission::Full, Permission::ReadCall));
    }

    #[test]
    fn at_least_lower_does_not_cover_higher() {
        assert!(!permission_at_least(Permission::Read, Permission::ReadCall));
        assert!(!permission_at_least(Permission::Read, Permission::Full));
        assert!(!permission_at_least(Permission::ReadCall, Permission::Full));
    }

    #[test]
    fn required_permission_read_class() {
        assert_eq!(required_permission("env_list"), Permission::Read);
        assert_eq!(required_permission("env_get_active"), Permission::Read);
        assert_eq!(required_permission("project_list"), Permission::Read);
        assert_eq!(required_permission("project_get"), Permission::Read);
        assert_eq!(required_permission("module_list"), Permission::Read);
        assert_eq!(required_permission("collection_list"), Permission::Read);
        assert_eq!(required_permission("endpoint_list"), Permission::Read);
        assert_eq!(required_permission("endpoint_get"), Permission::Read);
        assert_eq!(required_permission("endpoint_search"), Permission::Read);
        assert_eq!(required_permission("base_url_def_list"), Permission::Read);
        assert_eq!(
            required_permission("api_resolve_template"),
            Permission::Read
        );
        assert_eq!(
            required_permission("api_resolve_endpoint"),
            Permission::Read
        );
        assert_eq!(
            required_permission("api_validate_endpoint"),
            Permission::Read
        );
        assert_eq!(required_permission("api_parse_response"), Permission::Read);
        assert_eq!(required_permission("api_diff_responses"), Permission::Read);
        assert_eq!(
            required_permission("api_extract_jsonpath"),
            Permission::Read
        );
    }

    #[test]
    fn required_permission_read_call_class() {
        assert_eq!(
            required_permission("api_run_request"),
            Permission::ReadCall
        );
        assert_eq!(
            required_permission("api_call_endpoint"),
            Permission::ReadCall
        );
    }

    #[test]
    fn required_permission_defaults_unknown_to_full() {
        // Write tools — keep them out of the match so they fall here.
        assert_eq!(required_permission("projects_create"), Permission::Full);
        assert_eq!(required_permission("projects_update"), Permission::Full);
        assert_eq!(required_permission("projects_delete"), Permission::Full);
        assert_eq!(required_permission("endpoints_create"), Permission::Full);
        // Genuinely unknown name also defaults to Full (safe side).
        assert_eq!(required_permission("not_a_real_tool"), Permission::Full);
    }

    #[test]
    fn permission_string_roundtrip() {
        for p in [
            Permission::Read,
            Permission::ReadCall,
            Permission::Full,
        ]
        .iter()
        .copied()
        {
            assert_eq!(Permission::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn permission_string_rejects_garbage() {
        // `parse` is the domain-layer helper; my code only depends on
        // `Permission::as_str` being round-trippable, so the same cases
        // exercise the wrong-direction failure we care about.
        assert_eq!(Permission::parse("Read"), None); // wrong case
        assert_eq!(Permission::parse("admin"), None);
        assert_eq!(Permission::parse(""), None);
        assert_eq!(Permission::parse("full_control"), None);
    }

    #[test]
    fn ranks_form_total_order() {
        // Sanity: ranks must be strictly increasing.
        assert!(rank(Permission::Read) < rank(Permission::ReadCall));
        assert!(rank(Permission::ReadCall) < rank(Permission::Full));
    }

    // ============================================================
    // CapabilityPolicy — global MCP gate
    // ============================================================

    #[test]
    fn capability_policy_default_is_read_only() {
        // Safe-by-default. See docs/mcp-permission-policy.md.
        assert_eq!(CapabilityPolicy::default(), CapabilityPolicy::ReadOnly);
    }

    #[test]
    fn capability_policy_round_trip() {
        for p in [
            CapabilityPolicy::ReadOnly,
            CapabilityPolicy::SafeWrite,
            CapabilityPolicy::FullAccess,
        ] {
            assert_eq!(CapabilityPolicy::parse(p.as_str()), Some(p));
            assert_eq!(p.as_str(), p.as_str());
        }
        assert_eq!(CapabilityPolicy::parse("nope"), None);
        assert_eq!(CapabilityPolicy::parse(""), None);
        // Wrong case — wire format is snake_case and case-sensitive.
        assert_eq!(CapabilityPolicy::parse("Read_Only"), None);
        assert_eq!(CapabilityPolicy::parse("READ_ONLY"), None);
    }

    #[test]
    fn capability_policy_satisfies_matrix() {
        // ReadOnly
        assert!(CapabilityPolicy::ReadOnly.satisfies(Capability::Read));
        assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Create));
        assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Update));
        assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Delete));
        assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Send));

        // SafeWrite — covers Read/Create/Update but NOT Delete/Send.
        assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Read));
        assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Create));
        assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Update));
        assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Delete));
        assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Send));

        // FullAccess — covers everything.
        assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Read));
        assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Create));
        assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Update));
        assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Delete));
        assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Send));
    }

    #[test]
    fn capability_policy_serializes_to_snake_case_wire() {
        // Wire form the renderer reads back must stay snake_case even
        // though Rust's default serializer would emit "ReadOnly".
        assert_eq!(
            serde_json::to_string(&CapabilityPolicy::ReadOnly).unwrap(),
            "\"read_only\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityPolicy::SafeWrite).unwrap(),
            "\"safe_write\""
        );
        assert_eq!(
            serde_json::to_string(&CapabilityPolicy::FullAccess).unwrap(),
            "\"full_access\""
        );
    }

    #[test]
    fn capability_policy_deserializes_from_snake_case_wire() {
        let ro: CapabilityPolicy = serde_json::from_str("\"read_only\"").unwrap();
        assert_eq!(ro, CapabilityPolicy::ReadOnly);
        let sw: CapabilityPolicy = serde_json::from_str("\"safe_write\"").unwrap();
        assert_eq!(sw, CapabilityPolicy::SafeWrite);
        let fa: CapabilityPolicy = serde_json::from_str("\"full_access\"").unwrap();
        assert_eq!(fa, CapabilityPolicy::FullAccess);
        // Wrong-case must NOT round-trip — locks down the wire form.
        assert!(serde_json::from_str::<CapabilityPolicy>("\"Read_Only\"").is_err());
    }

    // ============================================================
    // resolve_capability — global × per-project conservative merge
    // ============================================================

    /// Construct a fresh in-temp pool. Mirrors the helper in
    /// `db::preferences::tests`.
    fn fresh_pool() -> crate::db::DbPool {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-cap-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        crate::db::init(&dir).expect("init")
    }

    #[tokio::test]
    async fn resolve_capability_global_default_is_read_only() {
        let pool = fresh_pool();
        let cap = resolve_capability(&pool, None).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::ReadOnly);
    }

    #[tokio::test]
    async fn resolve_capability_global_preference_takes_when_no_project() {
        let pool = fresh_pool();
        crate::db::with_tx(&pool, |c| {
            crate::db::preferences::set(c, crate::db::preferences::keys::MCP_POLICY, &"full_access".to_string())
        })
        .unwrap();
        let cap = resolve_capability(&pool, None).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::FullAccess);
    }

    #[tokio::test]
    async fn resolve_capability_takes_stricter_side_when_project_permits_more() {
        // global = ReadOnly, project = Full ⇒ effective: ReadOnly
        // (user's choice to globally block wins over per-project grant)
        let pool = fresh_pool();
        // FK requires the project to exist before permissions.set.
        let now = crate::domain::now_ms();
        crate::db::with_tx(&pool, |c| {
            crate::db::projects::create(
                c,
                &crate::domain::Project {
                    id: "p_full".to_string(),
                    name: "p_full".to_string(),
                    description: None,
                    base_url: None,
                    base_url_definitions: vec![],
                    default_base_url_definition_id: None,
                    color: crate::commands::default_color(),
                    created_at: now,
                    updated_at: now,
                },
            )
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::preferences::set(c, crate::db::preferences::keys::MCP_POLICY, &"read_only".to_string())
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::permissions::set(c, "p_full", Permission::Full)
        })
        .unwrap();
        let cap = resolve_capability(&pool, Some("p_full")).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::ReadOnly);
    }

    #[tokio::test]
    async fn resolve_capability_takes_stricter_side_when_global_permits_more() {
        // global = FullAccess, project = ReadCall ⇒ effective: SafeWrite
        // (per-project restrictive grant wins even when global is open)
        let pool = fresh_pool();
        // FK requires the project to exist before permissions.set.
        let now = crate::domain::now_ms();
        crate::db::with_tx(&pool, |c| {
            crate::db::projects::create(
                c,
                &crate::domain::Project {
                    id: "p_readcall".to_string(),
                    name: "p_readcall".to_string(),
                    description: None,
                    base_url: None,
                    base_url_definitions: vec![],
                    default_base_url_definition_id: None,
                    color: crate::commands::default_color(),
                    created_at: now,
                    updated_at: now,
                },
            )
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::preferences::set(c, crate::db::preferences::keys::MCP_POLICY, &"full_access".to_string())
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::permissions::set(c, "p_readcall", Permission::ReadCall)
        })
        .unwrap();
        let cap = resolve_capability(&pool, Some("p_readcall")).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::SafeWrite);
    }

    #[tokio::test]
    async fn resolve_capability_agree() {
        // both sides agree ⇒ that tier
        let pool = fresh_pool();
        // FK requires the project to exist before permissions.set.
        let now = crate::domain::now_ms();
        crate::db::with_tx(&pool, |c| {
            crate::db::projects::create(
                c,
                &crate::domain::Project {
                    id: "p_match".to_string(),
                    name: "p_match".to_string(),
                    description: None,
                    base_url: None,
                    base_url_definitions: vec![],
                    default_base_url_definition_id: None,
                    color: crate::commands::default_color(),
                    created_at: now,
                    updated_at: now,
                },
            )
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::preferences::set(c, crate::db::preferences::keys::MCP_POLICY, &"safe_write".to_string())
        })
        .unwrap();
        crate::db::with_tx(&pool, |c| {
            crate::db::permissions::set(c, "p_match", Permission::ReadCall)
        })
        .unwrap();
        let cap = resolve_capability(&pool, Some("p_match")).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::SafeWrite);
    }

    #[tokio::test]
    async fn resolve_capability_missing_project_id_falls_back_to_global() {
        // resolve_capability(Some("nonexistent")) — the per-project
        // lookup reads `default()` (ReadCall) when no row exists. So
        // global=FullAccess + missing project ⇒ effective: SafeWrite.
        let pool = fresh_pool();
        crate::db::with_tx(&pool, |c| {
            crate::db::preferences::set(c, crate::db::preferences::keys::MCP_POLICY, &"full_access".to_string())
        })
        .unwrap();
        let cap = resolve_capability(&pool, Some("nonexistent")).await.unwrap();
        assert_eq!(cap, CapabilityPolicy::SafeWrite);
    }
}