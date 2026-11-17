use super::*;

#[test]
fn at_least_self_always_holds() {
    assert!(permission_at_least(Permission::Read, Permission::Read));
    assert!(permission_at_least(
        Permission::ReadCall,
        Permission::ReadCall
    ));
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
    assert_eq!(required_permission("env_get_default"), Permission::Read);
    assert_eq!(required_permission("project_list"), Permission::Read);
    assert_eq!(required_permission("project_get"), Permission::Read);
    assert_eq!(required_permission("module_list"), Permission::Read);
    assert_eq!(required_permission("collection_list"), Permission::Read);
    assert_eq!(required_permission("endpoint_list"), Permission::Read);
    assert_eq!(required_permission("endpoint_get"), Permission::Read);
    assert_eq!(required_permission("endpoint_search"), Permission::Read);
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
    assert_eq!(required_permission("api_run_request"), Permission::ReadCall);
    assert_eq!(
        required_permission("api_call_endpoint"),
        Permission::ReadCall
    );
}

#[test]
fn required_permission_defaults_unknown_to_full() {
    // 写工具不进 `required_permission` 的匹配分支，靠默认值落到 `Full`。
    assert_eq!(required_permission("projects_create"), Permission::Full);
    assert_eq!(required_permission("projects_update"), Permission::Full);
    assert_eq!(required_permission("projects_delete"), Permission::Full);
    assert_eq!(required_permission("endpoints_create"), Permission::Full);
    // 真不存在的名字也取 `Full`：默认偏向安全一侧。
    assert_eq!(required_permission("not_a_real_tool"), Permission::Full);
}

#[test]
fn permission_string_roundtrip() {
    for p in [Permission::Read, Permission::ReadCall, Permission::Full]
        .iter()
        .copied()
    {
        assert_eq!(Permission::parse(p.as_str()), Some(p));
    }
}

#[test]
fn permission_string_rejects_garbage() {
    // `parse` 是领域层的辅助函数；调用方依赖的性质是 `Permission::as_str` 能往返，
    // 下面几个用例固定反方向的失败行为。
    assert_eq!(Permission::parse("Read"), None); // wrong case
    assert_eq!(Permission::parse("admin"), None);
    assert_eq!(Permission::parse(""), None);
    assert_eq!(Permission::parse("full_control"), None);
}

#[test]
fn ranks_form_total_order() {
    // 等级必须严格递增。
    assert!(rank(Permission::Read) < rank(Permission::ReadCall));
    assert!(rank(Permission::ReadCall) < rank(Permission::Full));
}

// ============================================================
// CapabilityPolicy —— 全局 MCP 闸门
// ============================================================

#[test]
fn capability_policy_default_is_read_only() {
    // 默认取最严的一档：没分类或叫不出名字的工具，都不许被当成允许。
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
    // 大小写写错就解析失败：线上格式是区分大小写的 snake_case。
    assert_eq!(CapabilityPolicy::parse("Read_Only"), None);
    assert_eq!(CapabilityPolicy::parse("READ_ONLY"), None);
}

#[test]
fn capability_policy_satisfies_matrix() {
    // ReadOnly 只覆盖 Read。
    assert!(CapabilityPolicy::ReadOnly.satisfies(Capability::Read));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Create));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Update));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Delete));
    assert!(!CapabilityPolicy::ReadOnly.satisfies(Capability::Send));

    // SafeWrite 覆盖 Read/Create/Update，不覆盖 Delete/Send。
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Read));
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Create));
    assert!(CapabilityPolicy::SafeWrite.satisfies(Capability::Update));
    assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Delete));
    assert!(!CapabilityPolicy::SafeWrite.satisfies(Capability::Send));

    // FullAccess 覆盖全部。
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Read));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Create));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Update));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Delete));
    assert!(CapabilityPolicy::FullAccess.satisfies(Capability::Send));
}

#[test]
fn capability_policy_serializes_to_snake_case_wire() {
    // 渲染进程读回的线上格式必须是 snake_case，而 Rust 默认序列化器会写出 "ReadOnly"。
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
    // 大小写不一致不许往返成功：这条断言固定线上格式。
    assert!(serde_json::from_str::<CapabilityPolicy>("\"Read_Only\"").is_err());
}

// ============================================================
// resolve_capability —— 全局能力档
// ============================================================

/// 建一个位于临时目录的新连接池，行为与 `db::preferences::tests` 里的同名辅助函数一致。
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
    let cap = resolve_capability(&pool).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::ReadOnly);
}

#[tokio::test]
async fn resolve_capability_returns_the_stored_global_policy() {
    let pool = fresh_pool();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"full_access".to_string(),
        )
    })
    .unwrap();
    let cap = resolve_capability(&pool).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::FullAccess);
}

#[tokio::test]
async fn resolve_capability_treats_an_unrecognised_stored_value_as_read_only() {
    // 手工写进去的未知档位不能被当成「放行」：解析失败回退到默认的 ReadOnly。
    let pool = fresh_pool();
    crate::db::with_tx(&pool, |c| {
        crate::db::preferences::set(
            c,
            crate::db::preferences::keys::MCP_POLICY,
            &"yolo".to_string(),
        )
    })
    .unwrap();
    let cap = resolve_capability(&pool).await.unwrap();
    assert_eq!(cap, CapabilityPolicy::ReadOnly);
}
