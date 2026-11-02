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
    // `api_run_collection` 逐个调用端点，每个端点都是一次出站请求，故与
    // `api_call_endpoint` 同等对待。它不以破坏性后缀结尾，名字里也不含
    // `api_call_endpoint`，故这条断言钉的是那一处显式排除。
    assert!(!filter_tools(
        "api_run_collection",
        CapabilityOverride::ReadOnly
    ));
}

#[test]
fn read_only_hides_the_dynamic_variable_tools_that_write_or_send() {
    // `dynamic_var_resolve` 会真的发出站请求（去数据源取数），与 `api_call_endpoint` 同级。
    assert!(!filter_tools(
        "dynamic_var_resolve",
        CapabilityOverride::ReadOnly
    ));
    // `dynamic_var_set_override` 以 `_override` 结尾，匹配不到破坏性后缀表，故显式处理。
    assert!(!filter_tools(
        "dynamic_var_set_override",
        CapabilityOverride::ReadOnly
    ));
    assert!(!filter_tools(
        "dynamic_var_create",
        CapabilityOverride::ReadOnly
    ));
    // 读工具照常可见。
    assert!(filter_tools(
        "dynamic_var_list",
        CapabilityOverride::ReadOnly
    ));
    assert!(filter_tools(
        "dynamic_var_get",
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
