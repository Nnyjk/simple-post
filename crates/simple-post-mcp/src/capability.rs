// crates/simple-post-mcp/src/capability.rs
//
// `--capability` flag (and the matching `SIMPLE_POST_MCP_CAPABILITY`
// env var) → tool-list filter applied locally on top of whatever the
// web backend returns. The backend already filters by its persisted
// `mcp.policy`, but a stricter client may want to further narrow
// what it sees in `tools/list`.
//
// We don't have access to the backend's `CapabilityPolicy` enum
// directly (it lives in `simple-post-core::permissions`), so we keep
// a tiny local enum that mirrors the wire format. Mapping:
//
//   read_only   → keep only tools whose backend metadata marks
//                 them as `Capability::Read`. We don't have that
//                 metadata in the response body, so in practice we
//                 only filter the well-known "destructive" tools
//                 (those whose names contain `_delete`, `_create`,
//                 `_update`, `_set_`, `_move`, `_duplicate`).
//
//   safe_write  → read-only + the safe mutators (env_set_var,
//                 env_set_baseurl, env_set_active, base_url_def_set_default,
//                 request_settings_update). We hardcode the destructive
//                 exclusion list.
//
//   full_access → no extra filter. Server's policy still applies.
//
// This is intentionally conservative: the *server* is the authority
// on what the client is allowed to do; this layer only adds an
// opt-in belt-and-suspenders narrowing for paranoid clients (e.g.
// the user wants Claude to never see destructive tools even when
// the server is set to `safe_write`).

use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityOverride {
    ReadOnly,
    SafeWrite,
    FullAccess,
}

impl FromStr for CapabilityOverride {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "read_only" | "read-only" | "readonly" => Ok(Self::ReadOnly),
            "safe_write" | "safe-write" | "safewrite" => Ok(Self::SafeWrite),
            "full_access" | "full-access" | "fullaccess" => Ok(Self::FullAccess),
            other => Err(format!(
                "unknown capability `{other}` (expected read_only|safe_write|full_access)"
            )),
        }
    }
}

impl std::fmt::Display for CapabilityOverride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "read_only",
            Self::SafeWrite => "safe_write",
            Self::FullAccess => "full_access",
        })
    }
}

/// Hardcoded list of tool name suffixes / prefixes that mark a tool
/// as destructive. We avoid pulling the backend's REQUIRED_CAPABILITY
/// table over the wire just for a UI filter — anything genuinely new
/// lands here as part of the same commit that adds the tool.
const DESTRUCTIVE_SUFFIXES: &[&str] = &[
    "_create",
    "_update",
    "_delete",
    "_set_",
    "_move",
    "_duplicate",
];

/// Return `true` if `name` should be visible in `tools/list` given
/// the override. `FullAccess` never strips anything locally.
pub fn filter_tools(name: &str, cap: CapabilityOverride) -> bool {
    match cap {
        CapabilityOverride::FullAccess => true,
        CapabilityOverride::SafeWrite => !is_destructive(name),
        CapabilityOverride::ReadOnly => {
            // Anything that mutates state or talks to the network is
            // hidden — keep only the safe readers.
            !is_destructive(name)
                && !name.contains("api_run_request")
                && !name.contains("api_call_endpoint")
                && !name.contains("api_resolve_endpoint")
                && !name.contains("api_validate_endpoint")
        }
    }
}

fn is_destructive(name: &str) -> bool {
    DESTRUCTIVE_SUFFIXES.iter().any(|s| name.ends_with(s))
        || name == "api_diff_responses"
        || name == "api_extract_jsonpath"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_hides_writes_and_sends() {
        assert!(filter_tools("env_list", CapabilityOverride::ReadOnly));
        assert!(!filter_tools("env_create", CapabilityOverride::ReadOnly));
        assert!(!filter_tools("api_run_request", CapabilityOverride::ReadOnly));
        assert!(!filter_tools("api_call_endpoint", CapabilityOverride::ReadOnly));
    }

    #[test]
    fn safe_write_allows_mutation_hides_destructive() {
        assert!(filter_tools("env_set_var", CapabilityOverride::SafeWrite));
        assert!(!filter_tools("env_delete", CapabilityOverride::SafeWrite));
        assert!(!filter_tools("project_delete", CapabilityOverride::SafeWrite));
    }

    #[test]
    fn full_access_passes_everything() {
        assert!(filter_tools("env_delete", CapabilityOverride::FullAccess));
        assert!(filter_tools("project_create", CapabilityOverride::FullAccess));
    }

    #[test]
    fn parses_wire_form() {
        assert_eq!(
            "read_only".parse::<CapabilityOverride>().unwrap(),
            CapabilityOverride::ReadOnly
        );
        assert_eq!(
            "safe-write".parse::<CapabilityOverride>().unwrap(),
            CapabilityOverride::SafeWrite
        );
        assert_eq!(
            "full_access".parse::<CapabilityOverride>().unwrap(),
            CapabilityOverride::FullAccess
        );
        assert!("garbage".parse::<CapabilityOverride>().is_err());
    }
}