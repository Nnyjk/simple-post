use super::*;

#[test]
fn read_only_hides_writes_and_sends() {
    assert!(filter_tools("env_list", CapabilityOverride::ReadOnly));
    assert!(!filter_tools("env_create", CapabilityOverride::ReadOnly));
    assert!(!filter_tools(
        "api_run_request",
        CapabilityOverride::ReadOnly
    ));
    assert!(!filter_tools(
        "api_call_endpoint",
        CapabilityOverride::ReadOnly
    ));
}

#[test]
fn safe_write_allows_mutation_hides_destructive() {
    assert!(filter_tools("env_set_var", CapabilityOverride::SafeWrite));
    assert!(!filter_tools("env_delete", CapabilityOverride::SafeWrite));
    assert!(!filter_tools(
        "project_delete",
        CapabilityOverride::SafeWrite
    ));
}

#[test]
fn full_access_passes_everything() {
    assert!(filter_tools("env_delete", CapabilityOverride::FullAccess));
    assert!(filter_tools(
        "project_create",
        CapabilityOverride::FullAccess
    ));
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
