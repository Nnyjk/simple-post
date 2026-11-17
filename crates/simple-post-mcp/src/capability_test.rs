use super::*;

use crate::mcp::REQUIRED_CAPABILITY;
use simple_post_core::permissions::Capability;

/// 过滤逻辑与能力表的双向一致性：表里每一行都必须被两档本地过滤如实执行。
///
/// `read_only` 放行 `Read`；`safe_write` 放行 `Read` / `Create` / `Update`（不看
/// `Send`，只有 `full_access` 覆盖网络调用）。这两个期望值在测试里独立写死，而不是
/// 转手调 `satisfies`——否则一致性断言会变成同义反复，过滤逻辑漂了它也跟着漂。
#[test]
fn local_filter_agrees_with_the_capability_table() {
    for (name, required) in REQUIRED_CAPABILITY {
        assert_eq!(
            filter_tools(name, CapabilityOverride::ReadOnly),
            *required == Capability::Read,
            "`{name}` 需要 {required:?}，read_only 档位的过滤结论与之不符"
        );
        assert_eq!(
            filter_tools(name, CapabilityOverride::SafeWrite),
            matches!(
                required,
                Capability::Read | Capability::Create | Capability::Update
            ),
            "`{name}` 需要 {required:?}，safe_write 档位的过滤结论与之不符"
        );
    }
}

/// 能力表里没有的名字在三档都不放行（fail closed）。
#[test]
fn unknown_tool_names_are_hidden_in_every_tier() {
    for cap in [
        CapabilityOverride::ReadOnly,
        CapabilityOverride::SafeWrite,
        CapabilityOverride::FullAccess,
    ] {
        assert!(
            !filter_tools("not_a_registered_tool", cap),
            "{cap} 档位放行了一个不在能力表里的名字"
        );
    }
}

#[test]
fn read_only_shows_reads_including_the_outbound_free_ones() {
    for name in [
        "env_list",
        "api_resolve_endpoint",
        "api_validate_endpoint",
        "api_diff_responses",
        "api_extract_jsonpath",
    ] {
        assert!(
            filter_tools(name, CapabilityOverride::ReadOnly),
            "read_only 档位隐藏了只读工具 `{name}`"
        );
    }
}

#[test]
fn read_only_hides_writes_and_sends() {
    for name in [
        "env_create",
        "spec_upsert",
        "dynamic_var_set_override",
        "api_run_request",
        "api_call_endpoint",
        "api_run_collection",
        "dynamic_var_resolve",
    ] {
        assert!(
            !filter_tools(name, CapabilityOverride::ReadOnly),
            "read_only 档位泄漏了要写状态或发外网的工具 `{name}`"
        );
    }
}

#[test]
fn safe_write_allows_mutation_and_hides_delete_and_send() {
    assert!(filter_tools("env_set_var", CapabilityOverride::SafeWrite));
    assert!(filter_tools("spec_upsert", CapabilityOverride::SafeWrite));
    assert!(!filter_tools("env_delete", CapabilityOverride::SafeWrite));
    assert!(!filter_tools(
        "project_delete",
        CapabilityOverride::SafeWrite
    ));
    assert!(!filter_tools(
        "api_call_endpoint",
        CapabilityOverride::SafeWrite
    ));
}

#[test]
fn full_access_passes_every_registered_tool() {
    for (name, _) in REQUIRED_CAPABILITY {
        assert!(
            filter_tools(name, CapabilityOverride::FullAccess),
            "full_access 档位隐藏了已注册工具 `{name}`"
        );
    }
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
