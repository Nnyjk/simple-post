//! Permission gating for Simple Post commands and MCP tools.
//!
//! Three-level rank: `Read` < `ReadCall` < `Full`. Every state-changing
//! surface — Tauri command or MCP tool handler — consults
//! [`required_permission`] for the minimum level the call demands, then
//! [`require`] against the project's stored level in the `permissions`
//! table.
//!
//! The [`Permission`] enum lives in `crate::domain` (mirrors
//! `src/types/domain.ts`); this module re-exports it so callers can do
//! `use crate::permissions::Permission` without depending on `domain`
//! directly. All the rank / table / gate logic stays here so it has one
//! place to evolve.

/// Re-export so `use crate::permissions::Permission` resolves to the
/// single canonical type defined in `crate::domain`. Keeps call sites
/// in commands.rs / mcp/ decoupled from `domain`'s exact location while
/// still sharing one definition across the crate.
pub use crate::domain::Permission;

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
}